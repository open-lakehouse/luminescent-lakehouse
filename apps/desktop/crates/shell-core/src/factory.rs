//! The agentic factory's API as an in-process service: `factory_server::router()` over
//! the built-in extension (the reference kind set), on an in-memory work graph seeded with
//! the demo plan, with no reconciler, so nothing is dispatched. The proxy serves it under
//! [`FACTORY`] without a port ([`crate::services::Proxy::with_in_process`]).
//!
//! It also serves the factory's MCP server (`workgraph-mcp`) at [`MCP_PATH`], for the
//! Agent view's own connection: the MCP Apps' views (plan review) and their app-only
//! tools (approve, return). It has no port either, so it takes no tokens: every request
//! is the person at the desktop, with an apps grant ([`desktop_grant`]).
//!
//! An external factory-server (a private composition, with its own extensions and its
//! reconciler) is bound by hand instead, `LUMINESCENT_SERVICES=factory=<url>`, and then
//! wins: [`in_process`] says whether the reference one is wanted at all.

use std::sync::Arc;

use factory_core::{FactoryBuilder, Registry, olai};
use workgraph_mcp::{Grant, ToolPolicy, WorkgraphMcp};

use crate::services::ServiceDirectory;

/// The key the control room addresses the factory by (factory-client's
/// `FACTORY_SERVICE_KEY`).
pub const FACTORY: &str = "factory";

/// Where the in-process factory serves MCP (the Agent view's `WORKGRAPH_MCP_URL`).
pub const MCP_PATH: &str = workgraph_mcp::http::PATH;

/// The grant of the desktop's side channel: every tool, the MCP Apps' ones too, as the
/// person at the desktop.
pub fn desktop_grant() -> Grant {
    let who = std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "desktop".into());
    Grant::new(who, ToolPolicy::all()).role("desktop").apps()
}

/// The registry the reference server's `just factory-server` loads: the profiles, repos
/// and environments the inspector offers.
const REGISTRY: &str = include_str!("../../../../../crates/factory-core/registry.example.yaml");

/// Whether the in-process factory is wanted: not when `overrides` (the modules bound by
/// hand) bind [`FACTORY`] to an external server.
pub fn in_process(overrides: &dyn ServiceDirectory) -> bool {
    overrides.resolve(FACTORY).is_none()
}

/// The reference factory's router, its demo plan seeded, with its MCP server at
/// [`MCP_PATH`].
pub async fn reference() -> factory_core::Result<axum::Router> {
    let builder = FactoryBuilder::new();
    let schema = builder.schema()?;
    let factory = builder
        .build(Arc::new(olai::in_memory(&schema)))?
        .with_registry(Registry::from_yaml(REGISTRY)?);
    factory_core::demo::seed(&factory).await?;
    let factory = Arc::new(factory);
    let mcp = WorkgraphMcp::new(factory.clone())
        .map_err(|e| factory_core::Error::internal(e.to_string()))?
        .with_grant(desktop_grant());
    Ok(factory_server::router(factory).merge(workgraph_mcp::http::local_router(mcp, MCP_PATH)))
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures::TryStreamExt;

    use super::*;
    use crate::services::{Placement, Proxy, ProxyRequest, StaticDirectory, StubDirectory};

    #[test]
    fn a_hand_binding_replaces_the_in_process_factory() {
        assert!(in_process(&StubDirectory));
        assert!(in_process(
            &StaticDirectory::parse("unity-catalog=http://localhost:8080").unwrap()
        ));
        assert!(!in_process(
            &StaticDirectory::parse("factory=https://factory.example").unwrap()
        ));
    }

    #[tokio::test]
    async fn the_proxy_serves_the_reference_demo_in_process() {
        let proxy = Proxy::new(Arc::new(StubDirectory))
            .with_in_process(FACTORY, reference().await.unwrap());
        let services = proxy.services();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].module_id, FACTORY);
        assert_eq!(services[0].placement, Placement::Local);

        // Connect's JSON unary call, as the control room's transport makes it (binary).
        let (head, body) = proxy
            .send(ProxyRequest {
                module_id: FACTORY.into(),
                method: "POST".into(),
                path: format!("{}/ListGraphs", factory_server::SERVICE_PATH),
                headers: vec![("content-type".into(), "application/json".into())],
                body: Some(b"{}".to_vec()),
            })
            .await
            .unwrap();
        let chunks: Vec<Bytes> = body.try_collect().await.unwrap();
        let text = String::from_utf8(chunks.concat()).unwrap();
        assert_eq!(head.status, 200, "{text}");
        assert!(text.contains("Typed work graph on olai-store"), "{text}");
    }

    /// One JSON-RPC message to the in-process `/mcp`, as the Agent view's connection
    /// sends it: the session's answer (the JSON of the SSE `data:` line), and the
    /// session id.
    async fn mcp(
        proxy: &Proxy,
        session: Option<&str>,
        message: serde_json::Value,
    ) -> (u16, Option<String>, Option<serde_json::Value>) {
        let mut headers = vec![
            ("content-type".into(), "application/json".into()),
            (
                "accept".into(),
                "application/json, text/event-stream".into(),
            ),
        ];
        if let Some(id) = session {
            headers.push(("mcp-session-id".into(), id.into()));
        }
        let (head, body) = proxy
            .send(ProxyRequest {
                module_id: FACTORY.into(),
                method: "POST".into(),
                path: MCP_PATH.into(),
                headers,
                body: Some(message.to_string().into_bytes()),
            })
            .await
            .unwrap();
        let id = head
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("mcp-session-id"))
            .map(|(_, v)| v.clone());
        if message.get("id").is_none() {
            return (head.status, id, None);
        }
        // The answer is the first `data:` event; the stream stays open after it.
        let mut body = body;
        let mut text = String::new();
        while let Some(chunk) = body.try_next().await.unwrap() {
            text.push_str(&String::from_utf8_lossy(&chunk));
            if let Some(line) = text.lines().find(|l| l.starts_with("data:") && l.len() > 6) {
                let json = serde_json::from_str(line.trim_start_matches("data:").trim()).unwrap();
                return (head.status, id, Some(json));
            }
        }
        panic!("no answer: {text}");
    }

    #[tokio::test]
    async fn the_in_process_factory_serves_plan_review_to_the_desktop() {
        let proxy = Proxy::new(Arc::new(StubDirectory))
            .with_in_process(FACTORY, reference().await.unwrap());
        // `initialize`, declaring MCP Apps as the Agent view's connection does.
        let (status, session, init) = mcp(
            &proxy,
            None,
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": { "extensions": {
                        "io.modelcontextprotocol/ui": { "mimeTypes": ["text/html;profile=mcp-app"] }
                    } },
                    "clientInfo": { "name": "luminescent-desktop", "version": "0" }
                }
            }),
        )
        .await;
        assert_eq!(status, 200, "{init:?}");
        assert_eq!(init.unwrap()["result"]["serverInfo"]["name"], "workgraph");
        let session = session.expect("a session id");
        mcp(
            &proxy,
            Some(&session),
            serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )
        .await;

        // The app-only tools are there for the desktop's grant.
        let (_, _, tools) = mcp(
            &proxy,
            Some(&session),
            serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        )
        .await;
        let tools = tools.unwrap();
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        for name in ["open_plan_review", "approve_revision", "return_plan"] {
            assert!(names.contains(&name), "{name} in {names:?}");
        }

        // The view, and the review of the demo plan.
        let (_, _, view) = mcp(
            &proxy,
            Some(&session),
            serde_json::json!({
                "jsonrpc": "2.0", "id": 3, "method": "resources/read",
                "params": { "uri": "ui://factory/plan" }
            }),
        )
        .await;
        let view = view.unwrap();
        let contents = &view["result"]["contents"][0];
        assert_eq!(contents["mimeType"], "text/html;profile=mcp-app");
        assert!(contents["text"].as_str().unwrap().contains("<html"));
        let (_, _, review) = mcp(
            &proxy,
            Some(&session),
            serde_json::json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": { "name": "open_plan_review", "arguments": {
                    "root": factory_root(&proxy).await
                } }
            }),
        )
        .await;
        let review = review.unwrap();
        assert_eq!(
            review["result"]["structuredContent"]["status"], "awaiting_user",
            "{review}"
        );
    }

    /// The demo plan's root, from ListGraphs.
    async fn factory_root(proxy: &Proxy) -> String {
        let (_, body) = proxy
            .send(ProxyRequest {
                module_id: FACTORY.into(),
                method: "POST".into(),
                path: format!("{}/ListGraphs", factory_server::SERVICE_PATH),
                headers: vec![("content-type".into(), "application/json".into())],
                body: Some(b"{}".to_vec()),
            })
            .await
            .unwrap();
        let chunks: Vec<Bytes> = body.try_collect().await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&chunks.concat()).unwrap();
        json["graphs"][0]["root"]["ref"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}
