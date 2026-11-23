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

/// On Postgres, when `FACTORY_TEST_DATABASE_URL` names one (`just
/// factory-test` starts it), each factory in a schema of its own. Skipped
/// otherwise, except in CI, which must run it.
#[cfg(feature = "postgres")]
#[tokio::test]
async fn olai_postgres() {
    let Ok(url) = std::env::var("FACTORY_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must run the conformance suite on Postgres: set FACTORY_TEST_DATABASE_URL"
        );
        eprintln!("skipped: set FACTORY_TEST_DATABASE_URL to run on Postgres");
        return;
    };
    conformance::run_all(|| {
        let url = url.clone();
        async move {
            let schema_name = format!("conformance_{}", uuid::Uuid::now_v7().simple());
            let admin = sqlx::PgPool::connect(&url).await.unwrap();
            sqlx::query(&format!("CREATE SCHEMA {schema_name}"))
                .execute(&admin)
                .await
                .unwrap();
            let search_path = format!("SET search_path TO {schema_name}");
            let pool = sqlx::postgres::PgPoolOptions::new()
                .after_connect(move |conn, _| {
                    let sql = search_path.clone();
                    Box::pin(async move {
                        sqlx::Executor::execute(conn, sql.as_str()).await?;
                        Ok(())
                    })
                })
                .connect(&url)
                .await
                .unwrap();
            let err = olai::postgres_check(&pool).await.unwrap_err();
            assert!(err.to_string().contains("isn't applied"), "{err}");
            olai::postgres_migrator().run(&pool).await.unwrap();
            olai::postgres_check(&pool).await.unwrap();
            let schema = Schema::builtin();
            Factory::new(
                Arc::new(olai::postgres(pool, &schema)),
                Extensions::builtin(),
            )
        }
    })
    .await;
}
