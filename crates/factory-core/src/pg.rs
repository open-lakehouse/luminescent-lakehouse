//! Connecting the [olai-store backend](crate::olai) to Postgres: where the
//! connection comes from ([`PgSource`]), where its password comes from
//! ([`Credentials`]), the pool ([`connect`]), a dedicated connection
//! ([`connection`]) and the migrations ([`migrate`]). The standalone server
//! and `luminescent-migrate factory` share it. With feature `lakebase`,
//! [`LakebaseCredentials`] supplies a Databricks service principal's OAuth
//! token.
//!
//! The store never migrates on connect. Apply [`migrate`] (or
//! `luminescent-migrate factory`) first, then check the schema with
//! [`olai::postgres_check`](crate::olai::postgres_check) at startup.

use async_trait::async_trait;

use sqlx::Connection;
pub use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};

use crate::{Error, Result};

#[cfg(feature = "lakebase")]
mod lakebase;
#[cfg(feature = "lakebase")]
pub use lakebase::LakebaseCredentials;

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
/// [`connect`], [`connection`] and [`migrate`] call [`apply`](Self::apply) on
/// the [`PgSource`]'s options, and [`connect`] then hands the new pool to
/// [`maintain`](Self::maintain). A rotating credential (a Lakebase OAuth
/// token, [`LakebaseCredentials`] with feature `lakebase`) implements both:
/// `apply` returns a token that's still good, fetching a new one when the
/// current one is due, and `maintain` keeps the pool current by handing it
/// new options ([`PgPool::set_connect_options`]) before the token expires.
/// Connections opened afterwards use the new token; open ones keep their
/// session.
#[async_trait]
pub trait Credentials: Send + Sync {
    /// `base` with the password new connections should use.
    async fn apply(&self, base: PgConnectOptions) -> Result<PgConnectOptions>;

    /// Keep `pool`, opened on `base` with [`apply`](Self::apply)'s password,
    /// current for as long as it's open. Static credentials have nothing to
    /// do (the default); a rotating one spawns a task that ends when the pool
    /// closes.
    fn maintain(&self, pool: &PgPool, base: PgConnectOptions) {
        let _ = (pool, base);
    }
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
/// [`Credentials::maintain`] keeps its password current.
pub async fn connect(source: &PgSource, credentials: &dyn Credentials) -> Result<PgPool> {
    let base = source.options()?;
    let options = credentials.apply(base.clone()).await?;
    let pool = PgPoolOptions::new()
        .connect_with(options)
        .await
        .map_err(|e| Error::internal(format!("connecting to {}: {e}", source.redacted())))?;
    credentials.maintain(&pool, base);
    Ok(pool)
}

/// A connection of its own on `source`, outside any pool, with
/// `credentials`' current password: for a session that has to stay put (a
/// `LISTEN` connection, say). A rotating credential's token only has to be
/// valid when the connection opens, so to reconnect, call this again: it
/// gets the current token, renewed if it was due.
pub async fn connection(source: &PgSource, credentials: &dyn Credentials) -> Result<PgConnection> {
    let options = credentials.apply(source.options()?).await?;
    PgConnection::connect_with(&options)
        .await
        .map_err(|e| Error::internal(format!("connecting to {}: {e}", source.redacted())))
}

/// The direct host of a Neon or Lakebase pooled host: the first label without
/// its `-pooler` suffix (`ep-x-pooler.region.aws.neon.tech` is
/// `ep-x.region.aws.neon.tech`). `None` when `host` isn't a pooled one.
pub fn direct_host(host: &str) -> Option<String> {
    let (first, rest) = match host.split_once('.') {
        Some((first, rest)) => (first, Some(rest)),
        None => (host, None),
    };
    let direct = first.strip_suffix("-pooler").filter(|d| !d.is_empty())?;
    Some(match rest {
        Some(rest) => format!("{direct}.{rest}"),
        None => direct.to_owned(),
    })
}

/// Fails when `source` is a pooled host (its first label ends in `-pooler`),
/// naming the direct host to use instead. A transaction pooler takes a
/// `LISTEN` and never delivers a notification, so a change feed's listener
/// has to connect directly. `NOTIFY` works through one.
pub fn refuse_pooler(source: &PgSource) -> Result<()> {
    let options = source.options()?;
    match direct_host(options.get_host()) {
        Some(direct) => Err(Error::invalid(format!(
            "{} is a connection pooler: LISTEN through it never delivers a notification; \
             listen on the direct host {direct} instead, or poll",
            redacted(&options)
        ))),
        None => Ok(()),
    }
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
    fn pooled_hosts_are_refused_with_their_direct_host() {
        assert_eq!(
            direct_host("ep-cool-42-pooler.us-east-2.aws.neon.tech").as_deref(),
            Some("ep-cool-42.us-east-2.aws.neon.tech")
        );
        assert_eq!(direct_host("db-pooler").as_deref(), Some("db"));
        for host in [
            "ep-cool-42.us-east-2.aws.neon.tech",
            "localhost",
            "pooler.example.com",
            "-pooler.x",
        ] {
            assert_eq!(direct_host(host), None, "{host}");
        }
        let pooled = PgSource::parse("postgres://u@ep-a-pooler.eu.neon.tech/db").unwrap();
        let err = refuse_pooler(&pooled).unwrap_err().to_string();
        assert!(err.contains("ep-a.eu.neon.tech"), "{err}");
        let direct = PgSource::parse("postgres://u@ep-a.eu.neon.tech/db").unwrap();
        refuse_pooler(&direct).unwrap();
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
