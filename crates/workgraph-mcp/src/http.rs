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

use crate::auth::{CallerAuth, require_grant};
use crate::server::WorkgraphMcp;

/// Where factory-server mounts the endpoint.
pub const PATH: &str = "/mcp";

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

/// The MCP endpoint at `path`, serving the callers `auth` grants. Merge it
/// into the factory's router.
pub fn router(
    server: WorkgraphMcp,
    auth: impl CallerAuth,
    path: &str,
    config: StreamableHttpServerConfig,
) -> Router {
    let mcp = StreamableHttpService::new(
        move || Ok(server.clone()),
        LocalSessionManager::default().into(),
        config,
    );
    Router::new()
        .nest_service(path, mcp)
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(auth) as Arc<dyn CallerAuth>,
            require_grant,
        ))
}

/// The MCP endpoint at `path` for an in-process mount that no network
/// reaches, serving every request as `server`'s fixed grant
/// ([`WorkgraphMcp::with_grant`]) without tokens. The caller's proxy sends
/// path-only requests, so a missing `Host` is taken as `localhost`.
///
/// Never serve it on a socket: anyone who can connect gets the grant.
pub fn local_router(server: WorkgraphMcp, path: &str) -> Router {
    let mcp = StreamableHttpService::new(
        move || Ok(server.clone()),
        LocalSessionManager::default().into(),
        config(&[]),
    );
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
