//! The standalone server's Postgres store: refused until migrated, migrated
//! by `--migrate`, and kept across restarts.
//!
//! On a database of its own on `FACTORY_TEST_DATABASE_URL`'s server (`just
//! factory-test` starts one). Skipped without it, except in CI.
#![cfg(feature = "bin")]

use std::sync::Arc;

use factory_core::pg::{PgPool, StaticCredentials};
use factory_core::{Extensions, Factory, Schema};
use factory_server::cli::open_backend;

/// A fresh database on the test server, and its URL.
async fn fresh_db() -> Option<String> {
    let Ok(base) = std::env::var("FACTORY_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must run the Postgres tests: set FACTORY_TEST_DATABASE_URL"
        );
        eprintln!("skipped: set FACTORY_TEST_DATABASE_URL to run on Postgres");
        return None;
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("factory_server_{}_{nanos}", std::process::id());
    let server = PgPool::connect(&base).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&server)
        .await
        .unwrap();
    server.close().await;
    let (prefix, _) = base.rsplit_once('/').unwrap();
    Some(format!("{prefix}/{name}"))
}

#[tokio::test]
async fn migrates_on_request_and_keeps_the_graph() {
    let Some(url) = fresh_db().await else { return };
    let schema = Schema::builtin();

    let err = open_backend(&url, false, &schema, &StaticCredentials)
        .await
        .err()
        .expect("an unmigrated store is refused");
    assert!(err.to_string().contains("isn't applied"), "{err}");

    let backend = open_backend(&url, true, &schema, &StaticCredentials)
        .await
        .unwrap();
    let factory = Arc::new(Factory::new(backend, Extensions::builtin()));
    let root = factory_core::demo::seed(&factory).await.unwrap();

    // A restart: migrating again is a no-op, and the plan is still there.
    let backend = open_backend(&url, true, &schema, &StaticCredentials)
        .await
        .unwrap();
    let factory = Factory::new(backend, Extensions::builtin());
    let graphs = factory.list_graphs().await.unwrap();
    assert_eq!(graphs.len(), 1);
    assert_eq!(graphs[0].root.r#ref, root);
}
