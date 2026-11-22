//! The factory's work-graph MCP server (`workgraph-mcp`) as a Node-API addon.
//!
//! The Neon Function's handler (`function/index.ts`) calls [`start`] once,
//! forwards every fetch `Request` through [`handle`], and calls [`shutdown`] on
//! SIGINT. Requests never touch a socket: [`handle`] drives the in-process axum
//! router with `oneshot`.
//!
//! Nothing lives between requests, since Neon evicts an isolate at any time
//! and runs several side by side:
//!
//! - MCP is served without sessions (`workgraph_mcp::http::hosted_router`):
//!   the 2026-07-28 protocol, and pre-2026 clients statelessly too.
//! - The work graph is on the branch's Postgres, in its own `workgraph`
//!   database (`luminescent-migrate workgraph` applies its schema; the addon
//!   only checks it).
//! - Callers authenticate with the branch's Neon Auth (an OAuth access token
//!   for the `workgraph` resource, or a web session token) or a service
//!   account's `olk_` API key (`workgraph_mcp::auth::olai`), and what each may
//!   do is configuration (`FACTORY_MCP_PRINCIPALS`, `FACTORY_MCP_APP_PRINCIPALS`).
//!   Nothing is minted here. The desktop's apps token (`FACTORY_MCP_APP_TOKENS`)
//!   is still accepted, until the desktop signs in to Neon Auth.
//!
//! A response streams when it's an SSE stream (a tool call's, with rmcp's
//! keep-alives every 15 s while a slow tool runs): [`handle`] returns its head
//! and a stream id, and the handler pulls the body with [`next_chunk`]. Other
//! responses come back whole.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use axum::Router;
use axum::body::{Body, BodyDataStream};
use axum::http::header;
use axum::routing::get;
use factory_core::{Factory, FactoryBuilder, Registry, olai};
use futures::StreamExt;
use http_body_util::BodyExt;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use tower::ServiceExt;
use workgraph_mcp::auth::olai::{OlaiAuth, OlaiConfig, identity_pool, metadata_router};
use workgraph_mcp::auth::{AnyOf, PrincipalAuth};
use workgraph_mcp::{Principals, Tokens, WorkgraphMcp, http};

/// The database on the branch the work graph lives in, unless
/// `WORKGRAPH_NEON_DATABASE` names another. Not `neondb`: olai-store's tables
/// and migration ledger are unqualified.
const DEFAULT_DATABASE: &str = "workgraph";

/// Connections per isolate, unless `WORKGRAPH_NEON_POOL_SIZE` says otherwise.
/// The pooled URL multiplexes them on the branch's PgBouncer.
const DEFAULT_POOL_SIZE: u32 = 5;

/// The router, `None` before [`start`] and after [`shutdown`].
static ROUTER: Mutex<Option<Router>> = Mutex::new(None);
/// The factory behind it, for [`seed_demo`].
static FACTORY: Mutex<Option<Arc<Factory>>> = Mutex::new(None);
static TRACING: OnceLock<()> = OnceLock::new();

/// Response bodies being streamed, by id. A body is taken out (`None`) while
/// a chunk is read and put back after, unless [`cancel_stream`] removed the
/// entry meanwhile, so one stream has one reader and a cancelled one ends.
type Streams = HashMap<u32, Option<BodyDataStream>>;
static STREAMS: OnceLock<Mutex<Streams>> = OnceLock::new();
static NEXT_STREAM: AtomicU32 = AtomicU32::new(1);

fn streams() -> &'static Mutex<Streams> {
    STREAMS.get_or_init(Default::default)
}

#[napi(object)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[napi(object)]
pub struct JsRequest {
    pub method: String,
    /// Path and query, e.g. `/mcp`.
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Buffer,
}

#[napi(object)]
pub struct JsResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    /// The whole body, when it isn't streamed.
    pub body: Option<Buffer>,
    /// The stream to read the body from with [`next_chunk`], when it is.
    pub stream: Option<u32>,
}

fn to_napi(e: impl std::fmt::Display) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

/// `url` with its database (the path) replaced by `database`. Keep in sync with
/// `luminescent_migrate::with_database`, which has the tests: this crate can't
/// run Rust tests.
fn with_database(url: &str, database: &str) -> Result<String, String> {
    let mut url = url::Url::parse(url).map_err(|e| format!("invalid database url: {e}"))?;
    if url.cannot_be_a_base() {
        return Err("invalid database url: no host".into());
    }
    url.set_path(database);
    Ok(url.into())
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Connect to Postgres, check the work graph's schema, and build the router.
///
/// Configuration comes from the environment:
///
/// - `DATABASE_URL` (pooled, serves requests) and `DATABASE_URL_UNPOOLED`
///   (direct, the schema check), which Neon sets for the branch, both pointed
///   at the `workgraph` database (`WORKGRAPH_NEON_DATABASE`).
/// - `NEON_AUTH_BASE_URL` and `NEON_AUTH_JWKS_URL`, which Neon sets for a
///   branch with Neon Auth, and `WORKGRAPH_RESOURCE` (`neon.ts`): the tokens
///   accepted. Neon Auth's users and `authz`'s API keys are read from
///   `DATABASE_URL`'s own database (`neondb`), not `workgraph`.
/// - `FACTORY_MCP_PRINCIPALS` / `FACTORY_MCP_APP_PRINCIPALS`: what each
///   principal may do (`workgraph_mcp::Principals::from_env`). Without any,
///   every authenticated caller is a 403.
/// - `FACTORY_MCP_APP_TOKENS`, interim: the desktop's apps token, until it
///   signs in to Neon Auth. `FACTORY_MCP_TOKENS` is no longer read.
/// - `FACTORY_CONTROL_ROOM_URL`, optional: where results link a plan for
///   approval.
/// - `FACTORY_REGISTRY_YAML`, optional: the registry `list_repos` and
///   `list_profiles` read (the YAML of `factory-server --registry`).
///
/// Migrations are never applied here. Idempotent.
#[napi]
pub async fn start() -> napi::Result<()> {
    TRACING.get_or_init(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "workgraph_mcp=info,workgraph_neon=info".into());
        let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    });
    if ROUTER.lock().map_err(to_napi)?.is_some() {
        return Ok(());
    }
    let database = env("WORKGRAPH_NEON_DATABASE").unwrap_or_else(|| DEFAULT_DATABASE.into());
    let url = env("DATABASE_URL").ok_or_else(|| {
        to_napi("set DATABASE_URL to the Neon branch's Postgres connection string")
    })?;
    let pooled = with_database(&url, &database).map_err(to_napi)?;
    let direct = match env("DATABASE_URL_UNPOOLED") {
        Some(direct) => with_database(&direct, &database).map_err(to_napi)?,
        None => pooled.clone(),
    };
    let app_tokens = Tokens::app_tokens_from_env().map_err(to_napi)?;
    let principals = Principals::from_env().map_err(to_napi)?;
    let need = |name: &str| {
        env(name).ok_or_else(|| {
            to_napi(format!(
                "set {name}: workgraph authenticates callers with the branch's Neon Auth"
            ))
        })
    };
    let issuer = need("NEON_AUTH_BASE_URL")?;
    let resource = need("WORKGRAPH_RESOURCE")?;
    let config = OlaiConfig::neon(&issuer, need("NEON_AUTH_JWKS_URL")?, &resource);
    // The users and keys are in the branch's main database: the original
    // DATABASE_URL, never the `workgraph` one.
    let identity = identity_pool(&url).map_err(to_napi)?;
    let neon_auth = OlaiAuth::postgres(&config, identity).map_err(to_napi)?;

    // The check reads the migration ledger over the direct connection, like
    // the other functions' startup checks; requests use the pooled URL.
    let check = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&direct)
        .await
        .map_err(|e| to_napi(format!("connecting to the {database} database: {e}")))?;
    let checked = olai::postgres_check(&check).await;
    check.close().await;
    checked.map_err(to_napi)?;

    let size = env("WORKGRAPH_NEON_POOL_SIZE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_POOL_SIZE);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(size)
        .connect_lazy(&pooled)
        .map_err(to_napi)?;

    let builder = FactoryBuilder::new();
    let schema = builder.schema().map_err(to_napi)?;
    let backend = Arc::new(olai::postgres(pool, &schema));
    let mut factory = builder.build(backend).map_err(to_napi)?;
    if let Some(yaml) = env("FACTORY_REGISTRY_YAML") {
        factory = factory.with_registry(Registry::from_yaml(&yaml).map_err(to_napi)?);
    }
    let factory = Arc::new(factory);
    let mut server = WorkgraphMcp::new(factory.clone()).map_err(to_napi)?;
    if let Some(url) = env("FACTORY_CONTROL_ROOM_URL") {
        server = server.with_control_room(url);
    }
    if env("FACTORY_MCP_TOKENS").is_some() {
        tracing::warn!(
            "FACTORY_MCP_TOKENS is ignored: callers are principals now (FACTORY_MCP_PRINCIPALS)"
        );
    }
    if principals.is_empty() {
        tracing::warn!("no FACTORY_MCP_PRINCIPALS: every authenticated caller is a 403");
    }
    tracing::info!(
        database,
        principals = principals.len(),
        app_tokens = app_tokens.len(),
        "workgraph started"
    );

    let auth = AnyOf {
        tokens: app_tokens,
        rest: PrincipalAuth::new(neon_auth, principals),
    };
    let router = http::hosted_router(server, auth, http::PATH)
        .route("/health", get(|| async { "ok\n" }))
        .merge(metadata_router(&resource, &issuer));
    *FACTORY.lock().map_err(to_napi)? = Some(factory);
    *ROUTER.lock().map_err(to_napi)? = Some(router);
    Ok(())
}

/// Seed the reference demo plan, returning its root ref: for the tests,
/// which have no factory.v1 API here to create a plan through. Only the
/// handler's JavaScript can call it, never a request.
#[napi]
pub async fn seed_demo() -> napi::Result<String> {
    let factory = FACTORY
        .lock()
        .map_err(to_napi)?
        .clone()
        .ok_or_else(|| to_napi("workgraph is not started"))?;
    factory_core::demo::seed(&factory).await.map_err(to_napi)
}

fn is_event_stream(parts: &axum::http::response::Parts) -> bool {
    parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"))
}

/// Serve one request through the router: an SSE response as a stream (read
/// it with [`next_chunk`]), any other buffered whole.
#[napi]
pub async fn handle(req: JsRequest) -> napi::Result<JsResponse> {
    let router = ROUTER
        .lock()
        .map_err(to_napi)?
        .clone()
        .ok_or_else(|| to_napi("workgraph is not started"))?;

    let mut builder = axum::http::Request::builder()
        .method(req.method.as_str())
        .uri(req.url.as_str());
    for h in &req.headers {
        builder = builder.header(h.name.as_str(), h.value.as_str());
    }
    let request = builder
        .body(Body::from(req.body.to_vec()))
        .map_err(to_napi)?;

    let response = router.oneshot(request).await.map_err(to_napi)?;
    let (parts, body) = response.into_parts();
    let headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            Some(Header {
                name: name.to_string(),
                value: value.to_str().ok()?.to_string(),
            })
        })
        .collect();
    if is_event_stream(&parts) {
        let id = NEXT_STREAM.fetch_add(1, Ordering::Relaxed);
        streams()
            .lock()
            .map_err(to_napi)?
            .insert(id, Some(body.into_data_stream()));
        return Ok(JsResponse {
            status: parts.status.as_u16(),
            headers,
            body: None,
            stream: Some(id),
        });
    }
    let body = body.collect().await.map_err(to_napi)?.to_bytes();
    Ok(JsResponse {
        status: parts.status.as_u16(),
        headers,
        body: Some(body.to_vec().into()),
        stream: None,
    })
}

/// The next chunk of a streamed body, or `None` once it ended (and the
/// stream is gone). An unknown or cancelled id reads as ended.
#[napi]
pub async fn next_chunk(stream: u32) -> napi::Result<Option<Buffer>> {
    let taken = streams()
        .lock()
        .map_err(to_napi)?
        .get_mut(&stream)
        .and_then(Option::take);
    let Some(mut body) = taken else {
        return Ok(None);
    };
    let next = body.next().await;
    let mut open = streams().lock().map_err(to_napi)?;
    match next {
        Some(Ok(chunk)) => {
            // Put it back, unless it was cancelled while we read.
            if let Some(slot) = open.get_mut(&stream) {
                *slot = Some(body);
            }
            Ok(Some(chunk.to_vec().into()))
        }
        Some(Err(e)) => {
            open.remove(&stream);
            Err(to_napi(e))
        }
        None => {
            open.remove(&stream);
            Ok(None)
        }
    }
}

/// Drop a streamed body the client stopped reading.
#[napi]
pub fn cancel_stream(stream: u32) -> napi::Result<()> {
    streams().lock().map_err(to_napi)?.remove(&stream);
    Ok(())
}

/// Stop serving. Dropping the router closes the connection pool, and the
/// open streams end.
#[napi]
pub async fn shutdown() -> napi::Result<()> {
    ROUTER.lock().map_err(to_napi)?.take();
    FACTORY.lock().map_err(to_napi)?.take();
    streams().lock().map_err(to_napi)?.clear();
    Ok(())
}
