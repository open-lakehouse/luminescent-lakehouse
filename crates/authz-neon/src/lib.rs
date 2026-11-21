//! The lakehouse's Cedar policy decision point (PDP) as a Node-API addon.
//!
//! The PDP itself is breakwater's `olai-pdp` (its `docs/pdp.md`): olai-auth's
//! wire API (`POST /v1/authorize`, `POST /v1/authorize/batch`, `GET /health`)
//! over Cedar policies and entities from the branch's `authz` and `neon_auth`
//! schemas. This crate hosts it on a Neon Function, configured for the branch:
//!
//! - the callers are the lakehouse's services, each asking about its own
//!   namespace, with its `AUTHZ_TOKEN_<SERVICE>` secret;
//! - users come from Neon Auth (`neon_auth."user"`);
//! - the policies are olai-pdp's built-in `Lakehouse` schema and baseline,
//!   then the function's `policies/` (the lakehouse's own and the services'
//!   staged schema fragments).
//!
//! The Neon Function's handler (`function/index.ts`) calls [`start`] once,
//! forwards every fetch `Request` through [`handle`], and calls [`shutdown`] on
//! SIGINT. Requests never touch a socket: [`handle`] drives the in-process axum
//! router with `oneshot`. Nothing runs between requests.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use axum::Router;
use axum::body::Body;
use http_body_util::BodyExt;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use olai_pdp::{Caller, Config};
use tower::ServiceExt;

/// The services that may call, by the suffix of their `AUTHZ_TOKEN_<SERVICE>`
/// secret, and the Cedar namespace each may ask about.
const CALLERS: &[(&str, &str)] = &[
    ("HEADWATERS", "Headwaters"),
    ("OPENSHARING", "OpenSharing"),
    ("UNITYCATALOG", "UnityCatalog"),
];

/// The router, `None` before [`start`] and after [`shutdown`].
static ROUTER: Mutex<Option<Router>> = Mutex::new(None);
/// The actions of the composed schema, set by [`start`].
static ACTIONS: Mutex<Vec<ActionRef>> = Mutex::new(Vec::new());
static TRACING: OnceLock<()> = OnceLock::new();

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

/// The branch's PDP configuration: olai-pdp's from the environment
/// (`DATABASE_URL`, `AUTHZ_ENTITY_TTL_MS`, …), with the lakehouse's callers,
/// Neon Auth users and the function's `policies_dir`.
fn config(policies_dir: String) -> Result<Config, olai_pdp::Error> {
    let mut config = Config::from_env()?;
    config.callers = CALLERS
        .iter()
        .map(|(name, namespace)| Caller::new(*name, *namespace))
        .collect();
    config.neon_auth = true;
    config.policy_dirs = vec![PathBuf::from(policies_dir)];
    Ok(config)
}

/// Load and validate the policies in `policies_dir` (strict mode), read the
/// callers' secrets (`AUTHZ_TOKEN_<SERVICE>`) and prepare the connection pool
/// to `DATABASE_URL` (connected on first use). Fails when a policy doesn't
/// validate, so a bad policy never serves. Idempotent.
#[napi]
pub async fn start(policies_dir: String) -> napi::Result<()> {
    TRACING.get_or_init(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "authz_neon=info,olai_pdp=info,olai_auth=info".into());
        let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    });
    if ROUTER.lock().map_err(to_napi)?.is_some() {
        return Ok(());
    }
    let config = config(policies_dir.clone()).map_err(to_napi)?;
    let service = olai_pdp::embed::start(&config).await.map_err(to_napi)?;
    *ACTIONS.lock().map_err(to_napi)? = service
        .actions()
        .iter()
        .map(|action| ActionRef {
            entity_type: action.entity_type.clone(),
            id: action.id.clone(),
        })
        .collect();
    *ROUTER.lock().map_err(to_napi)? = Some(service.router());
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
