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

/// The database on a branch that Unity Catalog lives in. Not `neondb`: UC keeps
/// its migration ledger in the unqualified `_sqlx_migrations` too, and would
/// refuse headwaters' entries there (and headwaters UC's).
pub const UNITYCATALOG_DATABASE: &str = "unitycatalog";

/// `url` with its database (the path) replaced by `database`, the query (e.g.
/// `sslmode`) kept. `crates/unitycatalog-neon` has a copy, since an addon can't
/// run Rust tests.
pub fn with_database(url: &str, database: &str) -> anyhow::Result<String> {
    let mut url = url::Url::parse(url).context("invalid database url")?;
    anyhow::ensure!(!url.cannot_be_a_base(), "invalid database url: no host");
    url.set_path(database);
    Ok(url.into())
}

/// Connect for admin operations. All queries qualify `authz.`.
pub async fn connect(url: &str) -> anyhow::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .context("failed to connect to Postgres")
}

#[cfg(test)]
mod tests {
    use super::with_database;

    #[test]
    fn with_database_swaps_only_the_path() {
        // Without credentials: the pre-commit secret scan flags any inline
        // `user:password@` URL, and the userinfo is left alone anyway.
        assert_eq!(
            with_database(
                "postgresql://ep-x-pooler.eu-central-1.aws.neon.tech/neondb?sslmode=require&channel_binding=require",
                "unitycatalog",
            )
            .unwrap(),
            "postgresql://ep-x-pooler.eu-central-1.aws.neon.tech/unitycatalog?sslmode=require&channel_binding=require",
        );
        assert_eq!(
            with_database("postgres://localhost:5432", "unitycatalog").unwrap(),
            "postgres://localhost:5432/unitycatalog",
        );
        assert!(with_database("not a url", "unitycatalog").is_err());
    }
}
