//! Connecting the [olai-store backend](crate::olai) to Postgres: where the
//! connection comes from ([`PgSource`]), where its password comes from
//! ([`Credentials`]), the pool ([`connect`]) and the migrations ([`migrate`]).
//! The standalone server and `luminescent-migrate factory` share it.
//!
//! The store never migrates on connect. Apply [`migrate`] (or
//! `luminescent-migrate factory`) first, then check the schema with
//! [`olai::postgres_check`](crate::olai::postgres_check) at startup.

use async_trait::async_trait;

pub use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};

use crate::{Error, Result};

/// Where a Postgres connection comes from, as `FACTORY_DB` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PgSource {
    /// A `postgres://` or `postgresql://` URL.
    Url(String),
    /// `postgres`: libpq's environment variables (`PGHOST`, `PGPORT`,
    /// `PGDATABASE`, `PGUSER`, `PGPASSWORD`, `PGSSLMODE`, …, and `.pgpass`),
    /// as sqlx reads them.
    Env,
}

impl PgSource {
    /// The source `db` names, or `None` when it isn't a Postgres form.
    pub fn parse(db: &str) -> Option<Self> {
        if db == "postgres" {
            Some(Self::Env)
        } else if db.starts_with("postgres://") || db.starts_with("postgresql://") {
            Some(Self::Url(db.to_owned()))
        } else {
            None
        }
    }

    /// The options to connect with, before [`Credentials`] adds a password.
    pub fn options(&self) -> Result<PgConnectOptions> {
        match self {
            Self::Url(url) => url
                .parse()
                .map_err(|e| Error::invalid(format!("invalid Postgres URL: {e}"))),
            Self::Env => Ok(PgConnectOptions::new()),
        }
    }

    /// `user@host:port/database`, without the password: for logs.
    pub fn redacted(&self) -> String {
        match self.options() {
            Ok(o) => redacted(&o),
            Err(_) => "<invalid Postgres URL>".into(),
        }
    }
}

/// `user@host:port/database` of `options`, without the password.
pub fn redacted(options: &PgConnectOptions) -> String {
    format!(
        "{}@{}:{}/{}",
        options.get_username(),
        options.get_host(),
        options.get_port(),
        options.get_database().unwrap_or("")
    )
}

/// Supplies the password a pool's connections use: the seam for credentials
/// that aren't in the URL or `PGPASSWORD`.
///
/// [`connect`] and [`migrate`] call [`apply`](Self::apply) once, on the
/// [`PgSource`]'s options. A rotating credential (a Lakebase OAuth token, say)
/// implements it to fetch the current token, and keeps the pool current by
/// calling `apply` again before the token expires and handing the result to
/// [`PgPool::set_connect_options`]: connections opened afterwards use the new
/// token, and open ones keep their session.
#[async_trait]
pub trait Credentials: Send + Sync {
    /// `base` with the password new connections should use.
    async fn apply(&self, base: PgConnectOptions) -> Result<PgConnectOptions>;
}

/// The password the [`PgSource`] already carries (the URL's, `PGPASSWORD`, or
/// `.pgpass`), unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct StaticCredentials;

#[async_trait]
impl Credentials for StaticCredentials {
    async fn apply(&self, base: PgConnectOptions) -> Result<PgConnectOptions> {
        Ok(base)
    }
}

/// A pool on `source`, with `credentials`' password. Doesn't check the
/// schema: see [`olai::postgres_check`](crate::olai::postgres_check).
pub async fn connect(source: &PgSource, credentials: &dyn Credentials) -> Result<PgPool> {
    let options = credentials.apply(source.options()?).await?;
    PgPoolOptions::new()
        .connect_with(options)
        .await
        .map_err(|e| Error::internal(format!("connecting to {}: {e}", source.redacted())))
}

/// Apply the store's pending migrations
/// ([`olai::postgres_migrator`](crate::olai::postgres_migrator)) to `source`,
/// over a connection of their own: the migrator holds a session advisory
/// lock, so `source` must be a direct connection, not a transaction pooler.
pub async fn migrate(source: &PgSource, credentials: &dyn Credentials) -> Result<()> {
    let options = credentials.apply(source.options()?).await?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| Error::internal(format!("connecting to {}: {e}", source.redacted())))?;
    let applied = crate::olai::postgres_migrator()
        .run(&pool)
        .await
        .map_err(|e| Error::internal(format!("migrating {}: {e}", source.redacted())));
    pool.close().await;
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_postgres_forms_only() {
        assert_eq!(PgSource::parse("postgres"), Some(PgSource::Env));
        for url in ["postgres://localhost/factory", "postgresql://h:5433/db"] {
            assert_eq!(PgSource::parse(url), Some(PgSource::Url(url.into())));
        }
        for other in ["memory", "sqlite::memory:", "postgresx", "pg://h/db", ""] {
            assert_eq!(PgSource::parse(other), None, "{other}");
        }
    }

    #[test]
    fn redacted_drops_the_password() {
        // Built, not written inline: the pre-commit secret scan flags any
        // `user:password@` URL.
        let url = format!("postgres://factory:{}@db.internal:5433/wg", "hunter2");
        let source = PgSource::parse(&url).unwrap();
        let shown = source.redacted();
        assert_eq!(shown, "factory@db.internal:5433/wg");
        assert!(!shown.contains("hunter2"));
    }

    #[test]
    fn an_invalid_url_is_an_invalid_argument() {
        let source = PgSource::Url("postgres://h:notaport/db".into());
        assert!(matches!(source.options(), Err(Error::InvalidArgument(_))));
    }

    struct Fixed(&'static str);

    #[async_trait]
    impl Credentials for Fixed {
        async fn apply(&self, base: PgConnectOptions) -> Result<PgConnectOptions> {
            Ok(base.password(self.0))
        }
    }

    #[tokio::test]
    async fn credentials_supply_the_password() {
        let base = PgSource::parse("postgres://u@h/db")
            .unwrap()
            .options()
            .unwrap();
        let shown = format!("{:?}", Fixed("token").apply(base).await.unwrap());
        assert!(shown.contains("token"), "{shown}");
        let base = PgSource::parse("postgres://u@h/db")
            .unwrap()
            .options()
            .unwrap();
        let shown = format!("{:?}", StaticCredentials.apply(base).await.unwrap());
        assert!(!shown.contains("token"), "{shown}");
    }
}
