//! The conformance suite on every olai-store backend we ship.

use std::sync::Arc;

use factory_core::{Extensions, Factory, Schema, conformance, olai};

#[tokio::test]
async fn olai_in_memory() {
    conformance::run_all(|| async {
        let schema = Schema::builtin();
        Factory::new(Arc::new(olai::in_memory(&schema)), Extensions::builtin())
    })
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn olai_sqlite() {
    conformance::run_all(|| async {
        let schema = Schema::builtin();
        let backend = olai::sqlite("sqlite::memory:", &schema).await.unwrap();
        Factory::new(Arc::new(backend), Extensions::builtin())
    })
    .await;
}

#[tokio::test]
async fn demo_seeds() {
    let schema = Schema::builtin();
    let f = Factory::new(Arc::new(olai::in_memory(&schema)), Extensions::builtin());
    let root = factory_core::demo::seed(&f).await.unwrap();
    let state = f.graph(&root).await.unwrap();
    assert!(state.approved.is_some());
    assert!(!state.drifted.is_empty(), "the demo drifts after approval");
    assert!(f.revisions(&root).await.unwrap().len() > 8);
}
