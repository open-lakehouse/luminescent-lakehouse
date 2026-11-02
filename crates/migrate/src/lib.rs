//! The `authz` schema: the permission data the functions on a branch share,
//! its migrations, and the operations of `luminescent-admin`.
//!
//! The schema lives in the branch's main database next to Neon Auth's
//! `neon_auth`, which it refers to. headwaters and the sharing server keep
//! their migration ledger in an unqualified `_sqlx_migrations` and refuse to
//! start when it holds migrations they don't know, so `authz` keeps its own,
//! `authz._sqlx_migrations`: [`migrate`] runs the migrator with
//! `search_path = authz`.

pub mod admin;
pub mod api_key;

use anyhow::Context;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// The `authz` migrations, embedded at compile time.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("migrations/authz");

/// Apply the pending `authz` migrations to the database at `url` (a direct
/// connection: the migrator holds a session advisory lock).
pub async fn migrate(url: &str) -> anyhow::Result<()> {
    let options: PgConnectOptions = url.parse().context("invalid database url")?;
    let setup = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .context("failed to connect to Postgres")?;
    sqlx::query("CREATE SCHEMA IF NOT EXISTS authz")
        .execute(&setup)
        .await?;
    setup.close().await;

    // Unqualified names resolve in `authz` only, the ledger included.
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.options([("search_path", "authz")]))
        .await
        .context("failed to connect to Postgres")?;
    MIGRATOR
        .run(&pool)
        .await
        .context("failed to run the authz migrations")?;
    pool.close().await;
    tracing::info!("authz migrations applied");
    Ok(())
}

/// Connect for admin operations. All queries qualify `authz.`.
pub async fn connect(url: &str) -> anyhow::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .context("failed to connect to Postgres")
}
