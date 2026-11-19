//! The agentic factory's API as an in-process service: `factory_server::router()` over
//! the built-in extension (the reference kind set), on an in-memory work graph seeded with
//! the demo plan, with no reconciler, so nothing is dispatched. The proxy serves it under
//! [`FACTORY`] without a port ([`crate::services::Proxy::with_in_process`]).
//!
//! An external factory-server (a private composition, with its own extensions and its
//! reconciler) is bound by hand instead, `LUMINESCENT_SERVICES=factory=<url>`, and then
//! wins: [`in_process`] says whether the reference one is wanted at all.

use std::sync::Arc;

use factory_core::{FactoryBuilder, Registry, olai};

use crate::services::ServiceDirectory;

/// The key the control room addresses the factory by (factory-client's
/// `FACTORY_SERVICE_KEY`).
pub const FACTORY: &str = "factory";

/// The registry the reference server's `just factory-server` loads: the profiles, repos
/// and environments the inspector offers.
const REGISTRY: &str = include_str!("../../../../../crates/factory-core/registry.example.yaml");

/// Whether the in-process factory is wanted: not when `overrides` (the modules bound by
/// hand) bind [`FACTORY`] to an external server.
pub fn in_process(overrides: &dyn ServiceDirectory) -> bool {
    overrides.resolve(FACTORY).is_none()
}

/// The reference factory's router, its demo plan seeded.
pub async fn reference() -> factory_core::Result<axum::Router> {
    let builder = FactoryBuilder::new();
    let schema = builder.schema()?;
    let factory = builder
        .build(Arc::new(olai::in_memory(&schema)))?
        .with_registry(Registry::from_yaml(REGISTRY)?);
    factory_core::demo::seed(&factory).await?;
    Ok(factory_server::router(Arc::new(factory)))
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
}
