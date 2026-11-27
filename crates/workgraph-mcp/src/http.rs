//! MCP over Streamable HTTP, behind a [`CallerAuth`].
//!
//! One endpoint, many clients: each agent session presents its own
//! credential, and the desktop's side channel its own. The `Host` allowlist
//! (loopback by default) guards against DNS rebinding, not against callers;
//! the [`CallerAuth`] does that: factory-server's [`Tokens`](crate::Tokens),
//! or the host identity provider's principals
//! ([`PrincipalAuth`](crate::auth::PrincipalAuth)).
//!
//! [`hosted_router`] serves the same tools from a host that keeps nothing
//! between requests (the `workgraph` Neon Function): no MCP sessions, and
//! POST only.
//!
//! [`roles_router`] publishes the server's [role catalog](crate::auth::Roles::catalog)
//! without a token: which roles it grants and the tools each reaches, what a
//! host builds a gateway route per role from.
//!
//! [`local_router`] is the exception: an endpoint no network reaches (the
//! desktop's in-process factory, called through its own proxy), serving one
//! fixed grant without tokens.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::response::Response;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::auth::{CallerAuth, RoleCatalog, require_grant};
use crate::server::WorkgraphMcp;

/// Where factory-server mounts the endpoint.
pub const PATH: &str = "/mcp";

/// Where a host publishes its role catalog. Outside [`PATH`], which the MCP
/// service owns, tokens and all.
pub const ROLES_PATH: &str = "/roles";

/// How often an open SSE stream is pinged, so proxies keep it open.
const SSE_KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The Streamable HTTP config: SSE keep-alive, and `allowed_hosts` for the
/// inbound `Host` check (rmcp's loopback-only default when empty).
pub fn config(allowed_hosts: &[String]) -> StreamableHttpServerConfig {
    let config = StreamableHttpServerConfig::default().with_sse_keep_alive(Some(SSE_KEEP_ALIVE));
    if allowed_hosts.is_empty() {
        config
    } else {
        config.with_allowed_hosts(allowed_hosts.iter().cloned())
    }
}

/// The config of a server that keeps nothing between requests: the stateless
/// protocol for every client (`legacy_session_mode: false`, so a pre-2026
/// client's `initialize` opens no session), and no `Host` check. That check
/// guards a loopback server against DNS rebinding; a public endpoint has the
/// tokens, and its hostname isn't known ahead (each Neon branch has its own).
pub fn hosted_config() -> StreamableHttpServerConfig {
    config(&[])
        .with_legacy_session_mode(false)
        .disable_allowed_hosts()
}

/// The MCP endpoint at `path` for a stateless host: [`hosted_config`], and
/// anything but `POST` answered `405` before `auth` is asked, since there's
/// no stream to open (`GET`) or session to end (`DELETE`).
pub fn hosted_router(server: WorkgraphMcp, auth: impl CallerAuth, path: &str) -> Router {
    router(server, auth, path, hosted_config()).layer(axum::middleware::from_fn(post_only))
}

async fn post_only(req: Request, next: Next) -> Response {
    if req.method() == Method::POST {
        return next.run(req).await;
    }
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST")],
        "Method Not Allowed",
    )
        .into_response()
}

/// The session store of a stateful endpoint: sessions never time out.
/// rmcp's default drops one after five idle minutes, and an agent's session
/// sits idle that long while it waits on a sub-agent. Its next call then
/// gets `404 Session not found`, which not every client recovers from.
pub fn sessions() -> Arc<LocalSessionManager> {
    let mut sessions = LocalSessionManager::default();
    sessions.session_config.keep_alive = None;
    Arc::new(sessions)
}

/// The MCP endpoint at `path`, serving the callers `auth` grants. Merge it
/// into the factory's router.
pub fn router(
    server: WorkgraphMcp,
    auth: impl CallerAuth,
    path: &str,
    config: StreamableHttpServerConfig,
) -> Router {
    let mcp = StreamableHttpService::new(move || Ok(server.clone()), sessions(), config);
    Router::new()
        .nest_service(path, mcp)
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(auth) as Arc<dyn CallerAuth>,
            require_grant,
        ))
}

/// `GET` [`ROLES_PATH`]: `catalog` as JSON, `{"roles": [{"name", "policy",
/// "tools"}]}`, without a token. It names no principal and no credential.
pub fn roles_router(catalog: RoleCatalog) -> Router {
    let body = Arc::new(serde_json::to_string(&catalog).unwrap_or_else(|_| "{}".into()));
    Router::new().route(
        ROLES_PATH,
        axum::routing::get(move || {
            let body = body.clone();
            async move {
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    body.as_str().to_owned(),
                )
            }
        }),
    )
}

/// The MCP endpoint at `path` for an in-process mount that no network
/// reaches, serving every request as `server`'s fixed grant
/// ([`WorkgraphMcp::with_grant`]) without tokens. The caller's proxy sends
/// path-only requests, so a missing `Host` is taken as `localhost`.
///
/// Never serve it on a socket: anyone who can connect gets the grant.
pub fn local_router(server: WorkgraphMcp, path: &str) -> Router {
    let mcp = StreamableHttpService::new(move || Ok(server.clone()), sessions(), config(&[]));
    Router::new()
        .nest_service(path, mcp)
        .layer(axum::middleware::from_fn(local_host))
}

async fn local_host(mut req: Request, next: Next) -> Response {
    if !req.headers().contains_key(header::HOST) {
        req.headers_mut()
            .insert(header::HOST, HeaderValue::from_static("localhost"));
    }
    next.run(req).await
}
