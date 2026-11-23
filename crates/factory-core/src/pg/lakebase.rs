//! [`LakebaseCredentials`]: a Databricks service principal's OAuth token as
//! the Postgres password, renewed before it expires.

use std::time::Duration;

use async_trait::async_trait;

use super::{Credentials, PgConnectOptions, PgPool};
use crate::databricks::DatabricksOAuth;
use crate::token::TokenSource;
use crate::{Error, Result};

/// A Lakebase database's password: an OAuth token of a Databricks service
/// principal ([`DatabricksOAuth`]: the workspace's `/oidc/v1/token`, client
/// credentials, scope `all-apis`, cached and renewed five minutes before it
/// expires).
///
/// A token only has to be valid when a connection opens, so open connections
/// keep their session past its expiry. So:
///
/// - [`apply`](Credentials::apply) puts the current token in the options. A
///   dedicated connection (a `LISTEN` connection, say) calls it on every
///   reconnect, through [`pg::connection`](super::connection), and gets a
///   token that's still good.
/// - [`maintain`](Credentials::maintain) spawns a task that asks for the
///   token every [`interval`](Self::with_interval) and hands the pool the new
///   options when it was renewed ([`PgPool::set_connect_options`]); a failed
///   renewal is tried again on the next tick. It holds the pool and stops
///   when the pool closes ([`PgPool::close`]).
///
/// Clones share the token.
#[derive(Clone, Debug)]
pub struct LakebaseCredentials {
    oauth: DatabricksOAuth,
    interval: Duration,
}

impl LakebaseCredentials {
    /// How often the pool's task asks for the token: well inside the five
    /// minutes before expiry when the token is renewed.
    pub const INTERVAL: Duration = Duration::from_secs(60);

    /// The credentials of the service principal `client_id` on the workspace
    /// `host` (`https://…`, or a bare host name, which means HTTPS).
    pub fn new(
        host: &str,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Result<Self> {
        DatabricksOAuth::new(host, client_id, client_secret).map(Self::with_oauth)
    }

    /// The credentials over `oauth`'s tokens (shared with its clones).
    pub fn with_oauth(oauth: DatabricksOAuth) -> Self {
        Self {
            oauth,
            interval: Self::INTERVAL,
        }
    }

    /// The credentials Databricks Apps inject: `DATABRICKS_HOST`,
    /// `DATABRICKS_CLIENT_ID` and `DATABRICKS_CLIENT_SECRET`.
    pub fn from_env() -> Result<Self> {
        DatabricksOAuth::from_env()
            .map(Self::with_oauth)
            .map_err(|e| Error::invalid(format!("Lakebase credentials: {e}")))
    }

    /// Ask for the token every `interval` while the pool is open.
    #[must_use]
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// The token endpoint.
    pub fn token_url(&self) -> &str {
        self.oauth.token_url()
    }

    /// Give `pool` each renewed token, until the pool closes.
    async fn keep_current(self, pool: PgPool, base: PgConnectOptions, mut last: Option<String>) {
        let closed = pool.close_event();
        tokio::pin!(closed);
        let mut tick = tokio::time::interval(self.interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await;
        loop {
            tokio::select! {
                _ = &mut closed => return,
                _ = tick.tick() => {}
            }
            match self.oauth.token().await {
                Ok(token) if last.as_deref() == Some(token.as_str()) => {}
                Ok(token) => {
                    pool.set_connect_options(base.clone().password(&token));
                    tracing::debug!("renewed the Lakebase token");
                    last = Some(token);
                }
                Err(error) => {
                    tracing::warn!(%error, retry = ?self.interval, "renewing the Lakebase token");
                }
            }
        }
    }
}

#[async_trait]
impl Credentials for LakebaseCredentials {
    async fn apply(&self, base: PgConnectOptions) -> Result<PgConnectOptions> {
        Ok(base.password(&self.oauth.token().await?))
    }

    fn maintain(&self, pool: &PgPool, base: PgConnectOptions) {
        // The pool opened with the token `apply` gave it: the one cached now.
        let pool = pool.clone();
        let this = self.clone();
        tokio::spawn(async move {
            let last = this.oauth.token().await.ok();
            this.keep_current(pool, base, last).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::databricks::fake::{Reply, serve};
    use crate::pg::PgPoolOptions;
    use tokio::time::Instant;

    fn base() -> PgConnectOptions {
        "postgres://sp@lakebase.invalid/factory".parse().unwrap()
    }

    fn password(options: &PgConnectOptions) -> String {
        // sqlx has no getter for the password; its Debug shows it.
        let shown = format!("{options:?}");
        let start = shown.find("password: Some(\"").expect("a password") + 16;
        shown[start..start + shown[start..].find('"').unwrap()].to_owned()
    }

    /// Wait (in real time) until `ok`, or fail after five seconds.
    async fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ok() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Credentials whose pool task looks every 50 ms, over a client that
    /// doesn't retry a failed request itself.
    fn credentials(host: &str) -> LakebaseCredentials {
        LakebaseCredentials::with_oauth(crate::databricks::without_retries(host, "id", "secret"))
            .with_interval(Duration::from_millis(50))
    }

    #[tokio::test]
    async fn apply_puts_the_cached_token_in_the_options() {
        let (endpoint, host) = serve(vec![Reply::Token("t1", 3600)]).await;
        let credentials = LakebaseCredentials::new(&host, "sp-id", "sp-secret").unwrap();
        assert_eq!(password(&credentials.apply(base()).await.unwrap()), "t1");
        assert_eq!(password(&credentials.apply(base()).await.unwrap()), "t1");
        assert_eq!(
            endpoint.calls(),
            1,
            "the second apply uses the cached token"
        );
        assert!(
            endpoint.requests.lock().unwrap()[0]
                .1
                .contains("scope=all-apis")
        );
    }

    #[tokio::test]
    async fn a_dedicated_connection_gets_a_fresh_token_once_it_is_due() {
        // Good for a minute: inside the renewal margin, so a reconnect renews it.
        let (endpoint, host) = serve(vec![Reply::Token("t1", 60), Reply::Token("t2", 3600)]).await;
        let credentials = credentials(&host);
        assert_eq!(password(&credentials.apply(base()).await.unwrap()), "t1");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(password(&credentials.apply(base()).await.unwrap()), "t2");
        assert_eq!(endpoint.calls(), 2);
    }

    #[tokio::test]
    async fn a_failed_fetch_is_an_error() {
        let (_endpoint, host) = serve(vec![Reply::Fail]).await;
        let err = credentials(&host).apply(base()).await.unwrap_err();
        assert!(err.to_string().contains("OAuth token"), "{err}");
    }

    #[tokio::test]
    async fn the_pool_gets_each_renewed_token_and_a_failure_is_retried() {
        // t1 is due at once (a minute left); the first renewal fails, the
        // next tick gets t2, which lasts.
        let (endpoint, host) = serve(vec![
            Reply::Token("t1", 60),
            Reply::Fail,
            Reply::Token("t2", 3600),
        ])
        .await;
        let credentials = credentials(&host);
        let options = credentials.apply(base()).await.unwrap();
        // Lazy: nothing connects, so no database is needed.
        let pool = PgPoolOptions::new().connect_lazy_with(options);
        assert_eq!(password(&pool.connect_options()), "t1");
        credentials.maintain(&pool, base());

        eventually("the renewed token", || {
            password(&pool.connect_options()) == "t2"
        })
        .await;
        assert_eq!(endpoint.calls(), 3, "t1, the failure, the retry");
        // A reconnect elsewhere shares the renewed token.
        assert_eq!(password(&credentials.apply(base()).await.unwrap()), "t2");
        assert_eq!(endpoint.calls(), 3);
    }

    #[tokio::test]
    async fn the_task_stops_when_the_pool_closes() {
        // Each token is due at once, so a live task keeps fetching.
        let (endpoint, host) = serve(vec![Reply::Token("t", 60)]).await;
        let credentials = credentials(&host);
        let options = credentials.apply(base()).await.unwrap();
        let pool = PgPoolOptions::new().connect_lazy_with(options);
        credentials.maintain(&pool, base());
        eventually("a renewal", || endpoint.calls() >= 3).await;

        pool.close().await;
        let after_close = endpoint.calls();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            endpoint.calls() <= after_close + 1,
            "renewals continued after close: {} > {after_close}",
            endpoint.calls()
        );
    }
}
