//! The Unity Catalog server as a Node-API addon.
//!
//! The Neon Function's handler (`function/index.ts`) calls [`start`] once,
//! forwards every fetch `Request` through [`handle`], and calls [`shutdown`] on
//! SIGINT. Requests never touch a socket: [`handle`] drives the in-process axum
//! router (`unitycatalog_server::embed::start`) with `oneshot`.
//!
//! The catalog is pure request/response: nothing runs between requests, so an
//! isolate can be evicted at any time.

use std::sync::{Mutex, OnceLock};

use axum::Router;
use axum::body::Body;
use http_body_util::BodyExt;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use tower::ServiceExt;
use unitycatalog_server::config::{Backend, Config, ConfigValue};
use unitycatalog_server::embed;

/// The database on the branch the catalog lives in, unless
/// `UNITYCATALOG_NEON_DATABASE` names another. Not Neon's `neondb`: its
/// migration ledger is the unqualified `_sqlx_migrations`, which headwaters
/// already keeps there.
const DEFAULT_DATABASE: &str = "unitycatalog";

/// The router, `None` before [`start`] and after [`shutdown`].
static ROUTER: Mutex<Option<Router>> = Mutex::new(None);
static TRACING: OnceLock<()> = OnceLock::new();

#[napi(object)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[napi(object)]
pub struct JsRequest {
    pub method: String,
    /// Path and query, e.g. `/api/2.1/unity-catalog/catalogs?max_results=10`.
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Buffer,
}

#[napi(object)]
pub struct JsResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: Buffer,
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

/// Point the Postgres backend's URLs at `database`.
///
/// Only the config's copies change, never the `DATABASE_URL` variable: the
/// authenticator reads Neon Auth's users and `authz` from `neondb` through
/// `auth.olai.database-url: { env: DATABASE_URL }` (`neon.ts`).
fn use_database(config: &mut Config, database: &str) -> Result<(), String> {
    let Backend::Postgres(pg) = &mut config.backend else {
        return Err("set DATABASE_URL to the Neon branch's Postgres connection string".into());
    };
    for slot in [&mut pg.url, &mut pg.migration_url] {
        if let Some(value) = slot.as_ref().and_then(ConfigValue::value) {
            *slot = Some(ConfigValue::Value(with_database(&value, database)?));
        }
    }
    Ok(())
}

/// Load the configuration, connect to Postgres and verify its schema.
///
/// Configuration comes from the environment (`UC_SERVER_CONFIG_YAML`, plus
/// `DATABASE_URL` / `DATABASE_URL_UNPOOLED`, which Neon sets for the branch:
/// the pooled URL serves requests, the direct one the schema check). Both are
/// pointed at the `unitycatalog` database (`UNITYCATALOG_NEON_DATABASE`).
/// Migrations are never applied here. Idempotent.
#[napi]
pub async fn start() -> napi::Result<()> {
    TRACING.get_or_init(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "unitycatalog=info,unitycatalog_neon=info".into());
        let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    });
    if ROUTER.lock().map_err(to_napi)?.is_some() {
        return Ok(());
    }
    let database = std::env::var("UNITYCATALOG_NEON_DATABASE")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| DEFAULT_DATABASE.into());
    let mut config = Config::from_env().map_err(to_napi)?;
    use_database(&mut config, &database).map_err(to_napi)?;
    let service = embed::start(&config).await.map_err(to_napi)?;
    *ROUTER.lock().map_err(to_napi)? = Some(service.router);
    tracing::info!(database, "unity catalog started");
    Ok(())
}

/// Serve one request through the router and buffer the whole response.
#[napi]
pub async fn handle(req: JsRequest) -> napi::Result<JsResponse> {
    let router = ROUTER
        .lock()
        .map_err(to_napi)?
        .clone()
        .ok_or_else(|| to_napi("unity catalog is not started"))?;

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
    let body = body.collect().await.map_err(to_napi)?.to_bytes();
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
    Ok(JsResponse {
        status: parts.status.as_u16(),
        headers,
        body: body.to_vec().into(),
    })
}

/// Stop serving. Dropping the router closes the connection pool.
#[napi]
pub async fn shutdown() -> napi::Result<()> {
    ROUTER.lock().map_err(to_napi)?.take();
    Ok(())
}
