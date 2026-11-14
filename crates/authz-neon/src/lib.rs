//! The lakehouse's Cedar policy decision point (PDP) as a Node-API addon.
//!
//! The services on a branch ask it whether a principal may perform an action
//! on a resource, over the wire format of `olai_auth::wire` (the services'
//! `olai_auth::HttpAuthorizer` is the client): `POST /v1/authorize` and
//! `POST /v1/authorize/batch`, authenticated with a per-service secret
//! (`AUTHZ_TOKEN_<SERVICE>`), and `GET /health`, open.
//!
//! Decisions are `olai_auth::CedarAuthorizer`'s, over the policies and schema
//! fragments bundled into the function's `policies/` (validated in strict mode
//! at [`start`]) and the entities [`loader::PgEntityLoader`] reads from the
//! branch's `authz` and `neon_auth` schemas.
//!
//! The Neon Function's handler (`function/index.ts`) calls [`start`] once,
//! forwards every fetch `Request` through [`handle`], and calls [`shutdown`] on
//! SIGINT. Requests never touch a socket: [`handle`] drives the in-process axum
//! router with `oneshot`. Nothing runs between requests.

mod http;
mod loader;

use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use http_body_util::BodyExt;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use olai_auth::CedarAuthorizer;
use tower::ServiceExt;

/// The router, `None` before [`start`] and after [`shutdown`].
static ROUTER: Mutex<Option<Router>> = Mutex::new(None);
/// The actions of the composed schema, set by [`start`].
static ACTIONS: Mutex<Vec<ActionRef>> = Mutex::new(Vec::new());
static TRACING: OnceLock<()> = OnceLock::new();

/// How long the loader reuses a principal's groups and grants unless
/// `AUTHZ_ENTITY_TTL_MS` says otherwise.
const DEFAULT_ENTITY_TTL: Duration = Duration::from_secs(2);

#[napi(object)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[napi(object)]
pub struct JsRequest {
    pub method: String,
    /// Path and query, e.g. `/v1/authorize`.
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

/// An action of the schema, e.g. `UnityCatalog::Action` `select`.
#[napi(object)]
#[derive(Clone)]
pub struct ActionRef {
    #[napi(js_name = "type")]
    pub entity_type: String,
    pub id: String,
}

fn to_napi(e: impl std::fmt::Display) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

/// The schema (every `*.cedarschema`, composed by [`compose_schema`]) and the
/// policies (every `*.cedar`, concatenated) in `dir`, in file name order.
fn read_policies(dir: &Path) -> Result<(String, String), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    let mut paths: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    let (mut fragments, mut policies) = (Vec::new(), String::new());
    for path in paths {
        let is_schema = match path.extension().and_then(|ext| ext.to_str()) {
            Some("cedarschema") => true,
            Some("cedar") => false,
            _ => continue,
        };
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        if is_schema {
            fragments.push((path.display().to_string(), text));
        } else {
            policies.push_str(&format!("// {}\n{text}\n", path.display()));
        }
    }
    if fragments.is_empty() || policies.is_empty() {
        return Err(format!(
            "{} needs at least one *.cedarschema and one *.cedar file",
            dir.display()
        ));
    }
    Ok((compose_schema(&fragments)?, policies))
}

/// One schema of the `(name, text)` fragments: each parsed on its own, then
/// merged namespace by namespace. Several fragments may declare the same
/// namespace (mangrove's entities, its generated actions and its Delta API's are
/// three `UnityCatalog` fragments), which one concatenated text may not; a type
/// or action declared twice is an error.
fn compose_schema(fragments: &[(String, String)]) -> Result<String, String> {
    use serde_json::{Map, Value};

    let mut merged: Map<String, Value> = Map::new();
    for (name, text) in fragments {
        let (fragment, warnings) = cedar_policy::SchemaFragment::from_cedarschema_str(text)
            .map_err(|e| format!("{name}: {e}"))?;
        for warning in warnings {
            tracing::debug!(%warning, fragment = %name, "Cedar schema warning");
        }
        let Value::Object(namespaces) = fragment.to_json_value().map_err(|e| format!("{name}: {e}"))?
        else {
            return Err(format!("{name}: not a schema"));
        };
        for (namespace, body) in namespaces {
            let Value::Object(body) = body else {
                return Err(format!("{name}: namespace {namespace:?} is not an object"));
            };
            let target = merged
                .entry(namespace.clone())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or("not an object")?;
            for (section, items) in body {
                match (target.get_mut(&section), items) {
                    (Some(Value::Object(existing)), Value::Object(items)) => {
                        for (key, item) in items {
                            if existing.insert(key.clone(), item).is_some() {
                                return Err(format!(
                                    "{name}: {namespace}::{key} ({section}) is declared twice"
                                ));
                            }
                        }
                    }
                    (None, items) => {
                        target.insert(section, items);
                    }
                    (Some(_), _) => {
                        return Err(format!(
                            "{name}: {namespace} {section} can't be merged"
                        ));
                    }
                }
            }
        }
    }
    cedar_policy::SchemaFragment::from_json_value(Value::Object(merged))
        .map_err(|e| e.to_string())?
        .to_cedarschema()
        .map_err(|e| e.to_string())
}

fn schema_actions(schema: &str) -> Result<Vec<ActionRef>, String> {
    let (schema, _) =
        cedar_policy::Schema::from_cedarschema_str(schema).map_err(|e| e.to_string())?;
    let mut actions: Vec<ActionRef> = schema
        .actions()
        .map(|uid| ActionRef {
            entity_type: uid.type_name().to_string(),
            id: uid.id().unescaped().to_string(),
        })
        .collect();
    actions.sort_by(|a, b| (&a.entity_type, &a.id).cmp(&(&b.entity_type, &b.id)));
    Ok(actions)
}

fn entity_ttl() -> Result<Duration, String> {
    match std::env::var("AUTHZ_ENTITY_TTL_MS") {
        Ok(ms) if !ms.is_empty() => u64::from_str(&ms)
            .map(Duration::from_millis)
            .map_err(|e| format!("invalid AUTHZ_ENTITY_TTL_MS {ms:?}: {e}")),
        _ => Ok(DEFAULT_ENTITY_TTL),
    }
}

/// Load and validate the policies in `policies_dir` (strict mode), read the
/// callers' secrets (`AUTHZ_TOKEN_<SERVICE>`) and prepare the connection pool
/// to `DATABASE_URL` (connected on first use). Fails when a policy doesn't
/// validate, so a bad policy never serves. Idempotent.
#[napi]
pub async fn start(policies_dir: String) -> napi::Result<()> {
    TRACING.get_or_init(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "authz_neon=info,olai_auth=info".into());
        let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    });
    if ROUTER.lock().map_err(to_napi)?.is_some() {
        return Ok(());
    }
    let (schema, policies) = read_policies(Path::new(&policies_dir)).map_err(to_napi)?;
    let actions = schema_actions(&schema).map_err(to_napi)?;
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            to_napi("set DATABASE_URL to the Neon branch's Postgres connection string")
        })?;
    let loader = loader::PgEntityLoader::new(&database_url, entity_ttl().map_err(to_napi)?)
        .map_err(to_napi)?;
    let authorizer = CedarAuthorizer::new(&schema, &policies, loader).map_err(to_napi)?;
    let callers = http::Callers::from_env();
    if callers.is_empty() {
        tracing::warn!("no AUTHZ_TOKEN_<SERVICE> is set: every authorization request is a 401");
    }
    let router = http::router(Arc::new(http::AppState {
        authorizer,
        callers,
    }));
    *ACTIONS.lock().map_err(to_napi)? = actions;
    *ROUTER.lock().map_err(to_napi)? = Some(router);
    tracing::info!(policies_dir, "authz started");
    Ok(())
}

/// The actions of the composed schema (after [`start`]).
#[napi]
pub fn actions() -> napi::Result<Vec<ActionRef>> {
    Ok(ACTIONS.lock().map_err(to_napi)?.clone())
}

/// Serve one request through the router and buffer the whole response.
#[napi]
pub async fn handle(req: JsRequest) -> napi::Result<JsResponse> {
    let router = ROUTER
        .lock()
        .map_err(to_napi)?
        .clone()
        .ok_or_else(|| to_napi("authz is not started"))?;

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

/// Stop serving. Dropping the router drops the connection pool.
#[napi]
pub async fn shutdown() -> napi::Result<()> {
    ROUTER.lock().map_err(to_napi)?.take();
    Ok(())
}
