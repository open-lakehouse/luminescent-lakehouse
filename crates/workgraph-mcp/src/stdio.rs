//! MCP over stdin and stdout: one client per process, whose grant is fixed
//! when the process starts (there's no token to present).

use rmcp::ServiceExt;

use crate::auth::Grant;
use crate::server::WorkgraphMcp;

/// Serve one client on stdin/stdout as `grant`, until it disconnects.
pub async fn serve_stdio(
    server: WorkgraphMcp,
    grant: Grant,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let running = server
        .with_grant(grant)
        .serve(rmcp::transport::stdio())
        .await?;
    running.waiting().await?;
    Ok(())
}
