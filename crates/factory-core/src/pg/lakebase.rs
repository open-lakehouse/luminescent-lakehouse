//! [`LakebaseCredentials`]: a Databricks service principal's OAuth token as
//! the Postgres password, renewed before it expires.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until};

use super::{Credentials, PgConnectOptions, PgPool};
use crate::{Error, Result};

/// A Lakebase database's password: an OAuth token of a Databricks service
/// principal, from the workspace's token endpoint (`/oidc/v1/token`, client
/// credentials, scope `all-apis`).
///
/// A token only has to be valid when a connection opens, so open connections
/// keep their session past its expiry. The credentials cache the token and
/// renew it [`margin`](Self::with_timing) before it expires:
///
/// - [`apply`](Credentials::apply) returns the cached token while it's
///   current, and fetches a new one once it's due. A dedicated connection (a
///   `LISTEN` connection, say) calls it on every reconnect, through
///   [`pg::connection`](super::connection), and gets a token that's still
///   good.
/// - [`maintain`](Credentials::maintain) spawns a task that renews the token
///   when it's due and hands the pool the new options
///   ([`PgPool::set_connect_options`]), retrying after a failure. It holds
///   the pool and stops when the pool closes ([`PgPool::close`]).
///
/// Clones share the token.
#[derive(Clone)]
pub struct LakebaseCredentials {
    inner: Arc<Inner>,
    margin: Duration,
    retry: Duration,
}

struct Inner {
    url: String,
    client_id: String,
    client_secret: String,
    http: reqwest::Client,
    token: Mutex<Option<Token>>,
}

#[derive(Clone)]
struct Token {
    secret: String,
    renew_at: Instant,
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

impl std::fmt::Debug for LakebaseCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LakebaseCredentials")
            .field("url", &self.inner.url)
            .field("client_id", &self.inner.client_id)
            .field("margin", &self.margin)
            .field("retry", &self.retry)
            .finish_non_exhaustive()
    }
}

impl LakebaseCredentials {
    /// How long before a token expires it's renewed.
    pub const MARGIN: Duration = Duration::from_secs(5 * 60);
    /// How long after a failed renewal the task tries again.
    pub const RETRY: Duration = Duration::from_secs(60);

    /// The credentials of the service principal `client_id` on the workspace
    /// `host` (`https://…`, or a bare host name, which means HTTPS).
    pub fn new(host: &str, client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        let host = host.trim_end_matches('/');
        let url = if host.starts_with("https://") || host.starts_with("http://") {
            format!("{host}/oidc/v1/token")
        } else {
            format!("https://{host}/oidc/v1/token")
        };
        Self {
            inner: Arc::new(Inner {
                url,
                client_id: client_id.into(),
                client_secret: client_secret.into(),
                http: reqwest::Client::new(),
                token: Mutex::new(None),
            }),
            margin: Self::MARGIN,
            retry: Self::RETRY,
        }
    }

    /// The credentials Databricks Apps inject: `DATABRICKS_HOST`,
    /// `DATABRICKS_CLIENT_ID` and `DATABRICKS_CLIENT_SECRET`.
    pub fn from_env() -> Result<Self> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        match (
            var("DATABRICKS_HOST"),
            var("DATABRICKS_CLIENT_ID"),
            var("DATABRICKS_CLIENT_SECRET"),
        ) {
            (Some(host), Some(id), Some(secret)) => Ok(Self::new(&host, id, secret)),
            _ => Err(Error::invalid(
                "Lakebase credentials need DATABRICKS_HOST, DATABRICKS_CLIENT_ID and \
                 DATABRICKS_CLIENT_SECRET",
            )),
        }
    }

    /// Renew a token `margin` before it expires (but never before half its
    /// lifetime), and retry a failed renewal after `retry`.
    pub fn with_timing(mut self, margin: Duration, retry: Duration) -> Self {
        self.margin = margin;
        self.retry = retry;
        self
    }

    /// The token endpoint.
    pub fn token_url(&self) -> &str {
        &self.inner.url
    }

    /// The cached token while it's current, a new one once it's due.
    async fn current(&self) -> Result<Token> {
        let mut cached = self.inner.token.lock().await;
        if let Some(token) = cached.as_ref()
            && Instant::now() < token.renew_at
        {
            return Ok(token.clone());
        }
        let token = self.fetch().await?;
        *cached = Some(token.clone());
        Ok(token)
    }

    /// A new token, cached. Unless another caller renewed it first: then
    /// that one.
    async fn renew(&self, due: Instant) -> Result<Token> {
        let mut cached = self.inner.token.lock().await;
        if let Some(token) = cached.as_ref()
            && token.renew_at > due
        {
            return Ok(token.clone());
        }
        let token = self.fetch().await?;
        *cached = Some(token.clone());
        Ok(token)
    }

    async fn fetch(&self) -> Result<Token> {
        let failed = |e: reqwest::Error| {
            Error::internal(format!(
                "fetching a Lakebase token from {}: {e}",
                self.inner.url
            ))
        };
        let response: TokenResponse = self
            .inner
            .http
            .post(&self.inner.url)
            .basic_auth(&self.inner.client_id, Some(&self.inner.client_secret))
            .form(&[("grant_type", "client_credentials"), ("scope", "all-apis")])
            .send()
            .await
            .map_err(failed)?
            .error_for_status()
            .map_err(failed)?
            .json()
            .await
            .map_err(failed)?;
        let lifetime = Duration::from_secs(response.expires_in);
        Ok(Token {
            secret: response.access_token,
            renew_at: Instant::now() + renew_after(lifetime, self.margin),
        })
    }

    /// Renew the token whenever it's due and give `pool` the new options,
    /// until the pool closes.
    async fn keep_current(self, pool: PgPool, base: PgConnectOptions) {
        let closed = pool.close_event();
        tokio::pin!(closed);
        let mut at = match self.inner.token.lock().await.as_ref() {
            Some(token) => token.renew_at,
            None => Instant::now(),
        };
        loop {
            tokio::select! {
                _ = &mut closed => return,
                _ = sleep_until(at) => {}
            }
            match self.renew(at).await {
                Ok(token) => {
                    pool.set_connect_options(base.clone().password(&token.secret));
                    tracing::debug!("renewed the Lakebase token");
                    at = token.renew_at;
                }
                Err(error) => {
                    tracing::warn!(%error, retry = ?self.retry, "renewing the Lakebase token");
                    at = Instant::now() + self.retry;
                }
            }
        }
    }
}

/// When to renew a token that's good for `lifetime`: `margin` before it
/// expires, but no sooner than half-way, so a short-lived token isn't
/// fetched in a loop.
fn renew_after(lifetime: Duration, margin: Duration) -> Duration {
    lifetime.saturating_sub(margin).max(lifetime / 2)
}

#[async_trait]
impl Credentials for LakebaseCredentials {
    async fn apply(&self, base: PgConnectOptions) -> Result<PgConnectOptions> {
        Ok(base.password(&self.current().await?.secret))
    }

    fn maintain(&self, pool: &PgPool, base: PgConnectOptions) {
        tokio::spawn(self.clone().keep_current(pool.clone(), base));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;

    use super::*;
    use crate::pg::PgPoolOptions;

    /// What the fake endpoint answers, in order; past the end, the last.
    #[derive(Clone)]
    enum Reply {
        Token(&'static str, u64),
        Fail,
    }

    /// A request's `Authorization` header and form body.
    type Request = (Option<String>, String);

    #[derive(Clone, Default)]
    struct Endpoint {
        replies: Arc<StdMutex<Vec<Reply>>>,
        requests: Arc<StdMutex<Vec<Request>>>,
    }

    impl Endpoint {
        fn calls(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    async fn token(
        State(endpoint): State<Endpoint>,
        headers: HeaderMap,
        body: String,
    ) -> Result<axum::Json<serde_json::Value>, StatusCode> {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let n = {
            let mut requests = endpoint.requests.lock().unwrap();
            requests.push((auth, body));
            requests.len()
        };
        let replies = endpoint.replies.lock().unwrap();
        match replies.get(n - 1).or(replies.last()).cloned().unwrap() {
            Reply::Token(secret, expires_in) => Ok(axum::Json(serde_json::json!({
                "access_token": secret,
                "token_type": "Bearer",
                "expires_in": expires_in,
            }))),
            Reply::Fail => Err(StatusCode::SERVICE_UNAVAILABLE),
        }
    }

    async fn serve(replies: Vec<Reply>) -> (Endpoint, String) {
        let endpoint = Endpoint {
            replies: Arc::new(StdMutex::new(replies)),
            ..Default::default()
        };
        let app = axum::Router::new()
            .route("/oidc/v1/token", post(token))
            .with_state(endpoint.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (endpoint, format!("http://{addr}"))
    }

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

    #[test]
    fn renews_a_margin_before_expiry_but_not_before_half_way() {
        let hour = Duration::from_secs(3600);
        assert_eq!(
            renew_after(hour, LakebaseCredentials::MARGIN),
            Duration::from_secs(55 * 60)
        );
        let short = Duration::from_secs(4 * 60);
        assert_eq!(
            renew_after(short, LakebaseCredentials::MARGIN),
            Duration::from_secs(2 * 60)
        );
        assert_eq!(renew_after(Duration::ZERO, Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn the_token_url_is_the_workspaces_oidc_endpoint() {
        for host in [
            "adb-1.azuredatabricks.net",
            "https://adb-1.azuredatabricks.net/",
        ] {
            assert_eq!(
                LakebaseCredentials::new(host, "id", "secret").token_url(),
                "https://adb-1.azuredatabricks.net/oidc/v1/token"
            );
        }
        let debug = format!("{:?}", LakebaseCredentials::new("h", "id", "hunter2"));
        assert!(!debug.contains("hunter2"), "{debug}");
    }

    #[tokio::test]
    async fn apply_fetches_with_client_credentials_and_caches_the_token() {
        let (endpoint, host) = serve(vec![Reply::Token("t1", 3600)]).await;
        let credentials = LakebaseCredentials::new(&host, "sp-id", "sp-secret");
        assert_eq!(password(&credentials.apply(base()).await.unwrap()), "t1");
        assert_eq!(password(&credentials.apply(base()).await.unwrap()), "t1");
        assert_eq!(
            endpoint.calls(),
            1,
            "the second apply uses the cached token"
        );

        let (auth, body) = endpoint.requests.lock().unwrap()[0].clone();
        // Basic base64("sp-id:sp-secret").
        assert_eq!(auth.as_deref(), Some("Basic c3AtaWQ6c3Atc2VjcmV0"));
        assert_eq!(body, "grant_type=client_credentials&scope=all-apis");
    }

    #[tokio::test]
    async fn a_dedicated_connection_gets_a_fresh_token_once_it_is_due() {
        // Good for 1 s, renewed half-way: a reconnect after that gets a new one.
        let (endpoint, host) = serve(vec![Reply::Token("t1", 1), Reply::Token("t2", 3600)]).await;
        let credentials = LakebaseCredentials::new(&host, "id", "secret")
            .with_timing(Duration::from_secs(1), Duration::from_millis(50));
        assert_eq!(password(&credentials.apply(base()).await.unwrap()), "t1");
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(password(&credentials.apply(base()).await.unwrap()), "t2");
        assert_eq!(endpoint.calls(), 2);
    }

    #[tokio::test]
    async fn a_failed_fetch_is_an_error() {
        let (_endpoint, host) = serve(vec![Reply::Fail]).await;
        let credentials = LakebaseCredentials::new(&host, "id", "secret");
        let err = credentials.apply(base()).await.unwrap_err();
        assert!(err.to_string().contains("Lakebase token"), "{err}");
    }

    #[tokio::test]
    async fn the_pool_gets_each_renewed_token_and_a_failure_is_retried() {
        // t1 is due half-way (0.5 s); the first renewal fails, the retry
        // (0.1 s later) gets t2, which lasts.
        let (endpoint, host) = serve(vec![
            Reply::Token("t1", 1),
            Reply::Fail,
            Reply::Token("t2", 3600),
        ])
        .await;
        let credentials = LakebaseCredentials::new(&host, "id", "secret")
            .with_timing(Duration::from_secs(1), Duration::from_millis(100));
        let options = credentials.apply(base()).await.unwrap();
        // Lazy: nothing connects, so no database is needed.
        let pool = PgPoolOptions::new().connect_lazy_with(options);
        credentials.maintain(&pool, base());
        assert_eq!(password(&pool.connect_options()), "t1");

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
        // Each token is due after 0.5 s, so a live task keeps fetching.
        let (endpoint, host) = serve(vec![Reply::Token("t", 1)]).await;
        let credentials = LakebaseCredentials::new(&host, "id", "secret")
            .with_timing(Duration::from_secs(1), Duration::from_millis(100));
        let options = credentials.apply(base()).await.unwrap();
        let pool = PgPoolOptions::new().connect_lazy_with(options);
        credentials.maintain(&pool, base());
        eventually("a renewal", || endpoint.calls() >= 2).await;

        pool.close().await;
        let after_close = endpoint.calls();
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(
            endpoint.calls() <= after_close + 1,
            "renewals continued after close: {} > {after_close}",
            endpoint.calls()
        );
    }
}
