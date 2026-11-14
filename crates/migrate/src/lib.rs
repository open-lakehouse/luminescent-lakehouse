//! The `authz` schema: the permission data the functions on a branch share,
//! its migrations, and the operations of `luminescent-admin`.
//!
//! The schema lives in the branch's main database next to Neon Auth's
//! `neon_auth`, which it refers to. Its tables are olai-auth's
//! ([`olai_auth::MIGRATOR`]: service accounts, API keys, groups, memberships,
//! grants and the `authz_reader` role), plus the lakehouse's own
//! ([`MIGRATOR`]: GitHub OIDC trust, and `authz_reader`'s access to Neon Auth's
//! users).
//!
//! headwaters and the sharing server keep their migration ledger in an
//! unqualified `_sqlx_migrations` and refuse to start when it holds migrations
//! they don't know, so each migrator here keeps its own: olai-auth's in
//! `authz._sqlx_migrations`, the lakehouse's in `luminescent._sqlx_migrations`.

pub mod admin;
pub mod api_key;

use anyhow::Context;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// The schema of the permission data.
pub const AUTHZ_SCHEMA: &str = "authz";

/// The schema of [`MIGRATOR`]'s ledger. Its tables live in [`AUTHZ_SCHEMA`].
pub const LEDGER_SCHEMA: &str = "luminescent";

/// The lakehouse's additions to the `authz` schema, applied after olai-auth's.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("migrations/luminescent");

async fn connect_direct(options: &PgConnectOptions) -> anyhow::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .context("failed to connect to Postgres")
}

/// Apply the pending `authz` migrations to the database at `url` (a direct
/// connection: the migrators hold a session advisory lock): olai-auth's, then
/// the lakehouse's.
///
/// A schema from before olai-auth owned the migrations (its ledger holds
/// `0001_init`) is refused; [`reset`] it first.
pub async fn migrate(url: &str) -> anyhow::Result<()> {
    let options: PgConnectOptions = url.parse().context("invalid database url")?;
    let pool = connect_direct(&options).await?;
    olai_auth::migrate(&pool, AUTHZ_SCHEMA)
        .await
        .map_err(|e| anyhow::anyhow!(e))
        .context(
            "failed to run olai-auth's authz migrations (an authz schema from before \
             them needs `luminescent-migrate authz --reset` first)",
        )?;
    pool.close().await;

    let setup = connect_direct(&options).await?;
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {LEDGER_SCHEMA}"))
        .execute(&setup)
        .await?;
    setup.close().await;
    // Unqualified names, the ledger's, resolve in the ledger schema; the
    // migrations qualify `authz.`.
    let pool = connect_direct(&options.options([("search_path", LEDGER_SCHEMA)])).await?;
    MIGRATOR
        .run(&pool)
        .await
        .context("failed to run the lakehouse's authz migrations")?;
    pool.close().await;
    tracing::info!("authz migrations applied");
    Ok(())
}

/// Drop the `authz` schema and the lakehouse's ledger, with every service
/// account, API key, GitHub trust rule, group and grant in them. For a branch
/// whose `authz` predates olai-auth's migrations; [`migrate`] it afterwards.
pub async fn reset(url: &str) -> anyhow::Result<()> {
    let pool = connect(url).await?;
    sqlx::raw_sql(&format!(
        "DROP SCHEMA IF EXISTS {AUTHZ_SCHEMA} CASCADE; DROP SCHEMA IF EXISTS {LEDGER_SCHEMA} CASCADE"
    ))
    .execute(&pool)
    .await
    .context("failed to drop the authz schema")?;
    pool.close().await;
    tracing::warn!("authz schema dropped");
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
