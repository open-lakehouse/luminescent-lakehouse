//! `luminescent-migrate factory`: factory-server's Postgres store, against a
//! real Postgres.
//!
//! `LUMINESCENT_TEST_DATABASE_URL` points at a server the tests may create
//! databases on (`just test-migrate` starts one); each test gets its own. Unset,
//! the tests skip.

use luminescent_migrate::{connect, migrate_factory};
use uuid::Uuid;

/// A fresh database on the test server, and its URL.
async fn fresh_db() -> Option<String> {
    let Ok(base) = std::env::var("LUMINESCENT_TEST_DATABASE_URL") else {
        eprintln!("LUMINESCENT_TEST_DATABASE_URL is not set; skipping");
        return None;
    };
    let server = connect(&base).await.unwrap();
    let name = format!("factory_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&server)
        .await
        .unwrap();
    server.close().await;
    Some(format!("{}/{name}", base.rsplit_once('/').unwrap().0))
}

#[tokio::test]
async fn migrates_the_store_and_reapplies_as_a_no_op() {
    let Some(url) = fresh_db().await else {
        return;
    };
    let pool = connect(&url).await.unwrap();
    let err = factory_core::olai::postgres_check(&pool).await.unwrap_err();
    assert!(err.to_string().contains("isn't applied"), "{err}");

    migrate_factory(&url).await.unwrap();
    factory_core::olai::postgres_check(&pool).await.unwrap();
    migrate_factory(&url).await.unwrap();
    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        applied,
        factory_core::olai::postgres_migrator().iter().count() as i64
    );
    pool.close().await;
}

#[tokio::test]
async fn refuses_a_store_that_isnt_postgres() {
    for db in ["memory", "sqlite::memory:", "mysql://h/db"] {
        let err = migrate_factory(db).await.unwrap_err();
        assert!(err.to_string().contains("FACTORY_DB"), "{db}: {err}");
    }
}
