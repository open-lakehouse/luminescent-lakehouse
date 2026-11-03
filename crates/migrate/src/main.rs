//! `luminescent-migrate <service>`: apply a hosted service's pending database
//! migrations, then exit. `authz` is the permission data the services share
//! (see the library docs).
//!
//! The Neon Functions never migrate; they only check the schema at startup. This
//! binary runs once per schema change, over the branch's *direct* connection
//! string (the migrators hold a session advisory lock, which the pooled
//! `DATABASE_URL` can't keep). It links the same pinned service crates as the
//! function addons, so the schema it applies is the one they expect.

use anyhow::{Context, bail};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    match std::env::args().nth(1).as_deref() {
        Some("headwaters") => headwaters().await,
        Some("opensharing") => opensharing().await,
        Some("unitycatalog") => unitycatalog().await,
        Some("authz") => authz().await,
        _ => bail!("usage: luminescent-migrate <headwaters|opensharing|unitycatalog|authz>"),
    }
}

/// Over `DATABASE_URL`, which scripts/migrate.sh points at the direct URL.
async fn authz() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL")
        .context("set DATABASE_URL to the branch's direct connection string")?;
    luminescent_migrate::migrate(&url).await
}

/// Reads the same `HEADWATERS__*` / `DATABASE_URL` configuration as the function.
async fn headwaters() -> anyhow::Result<()> {
    let cfg = headwaters::config::Config::load(None::<&str>).context("invalid configuration")?;
    headwaters::migrate(cfg).await
}

/// Reads the configuration the function does (`Config::from_env`) and migrates
/// over its migration URL (`DATABASE_URL_UNPOOLED`), falling back to the main URL.
async fn opensharing() -> anyhow::Result<()> {
    use opensharing_server::config::DatabaseConfig;
    use opensharing_server::store::{postgres, postgres_migrator};

    let config = opensharing_server::Config::from_env().map_err(anyhow::Error::msg)?;
    let DatabaseConfig::Postgres {
        url, migration_url, ..
    } = &config.database
    else {
        bail!("set DATABASE_URL to the Neon branch's Postgres connection string");
    };
    let url = migration_url
        .as_ref()
        .and_then(|u| u.value())
        .or_else(|| url.value())
        .context("database url is not set")?;
    let pool = postgres::connect(&url, 1).await?;
    postgres::migrate(&postgres_migrator(), &pool).await?;
    tracing::info!("migrations applied");
    Ok(())
}

/// Reads the configuration the function does (`Config::from_env`), moved to the
/// `unitycatalog` database as the function does, and migrates over its
/// migration URL (`DATABASE_URL_UNPOOLED`), falling back to the main URL.
/// Needs no KEK: migrations don't touch secrets.
async fn unitycatalog() -> anyhow::Result<()> {
    use unitycatalog_server::config::{Backend, Config, ConfigValue};

    let mut config = Config::from_env().map_err(anyhow::Error::msg)?;
    let Backend::Postgres(pg) = &mut config.backend else {
        bail!("set DATABASE_URL to the Neon branch's Postgres connection string");
    };
    for slot in [&mut pg.url, &mut pg.migration_url] {
        if let Some(url) = slot.as_ref().and_then(ConfigValue::value) {
            let url = luminescent_migrate::with_database(
                &url,
                luminescent_migrate::UNITYCATALOG_DATABASE,
            )?;
            *slot = Some(ConfigValue::Value(url));
        }
    }
    unitycatalog_server::embed::migrate(config).await?;
    tracing::info!("migrations applied");
    Ok(())
}
