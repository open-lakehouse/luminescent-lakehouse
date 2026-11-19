//! MCP over Streamable HTTP, behind the bearer-token check.
//!
//! One endpoint, many clients: each agent session presents its own token,
//! and the desktop's side channel its own. The `Host` allowlist (loopback by
//! default) guards against DNS rebinding, not against callers; the tokens do
//! that.
//!
//! [`local_router`] is the exception: an endpoint no network reaches (the
//! desktop's in-process factory, called through its own proxy), serving one
//! fixed grant without tokens.

use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::auth::{Tokens, require_token};
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

/// The MCP endpoint at `path`, accepting `tokens`. Merge it into the
/// factory's router.
pub fn router(
    server: WorkgraphMcp,
    tokens: Tokens,
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
        .layer(axum::middleware::from_fn_with_state(tokens, require_token))
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
