//! `luminescent-migrate <service>`: apply a hosted service's pending database
//! migrations, then exit.
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
        _ => bail!("usage: luminescent-migrate <headwaters|opensharing>"),
    }
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
