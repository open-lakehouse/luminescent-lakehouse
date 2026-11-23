//! [`DatabricksOAuth`]: a Databricks service principal's OAuth token, from
//! the workspace's token endpoint, cached and renewed before it expires.
//!
//! The token itself is trestle's: olai-http's Databricks unified auth
//! (`oauth-m2m`), which caches it, renews it five minutes before it expires
//! and retries a failed request. This module names the factory's uses of it:
//! the Lakebase password
//! ([`pg::LakebaseCredentials`](crate::pg::LakebaseCredentials), feature
//! `lakebase`), the bearer token for hosted Omnigent, and the workspace's
//! SCIM API all take a token from one.

use std::sync::Arc;

use async_trait::async_trait;
use olai_http::databricks::{DatabricksBuilder, DatabricksConfig, DatabricksConfigKey};
use olai_http::{ClientOptions, RetryConfig};

use crate::token::TokenSource;
use crate::{Error, Result};

/// The variable naming the workspace, as Databricks Apps inject it.
pub const HOST_ENV: &str = "DATABRICKS_HOST";

/// The scope the factory's tokens carry: every workspace API (Lakebase,
/// Omnigent, SCIM).
pub const SCOPE: &str = "all-apis";

/// An OAuth token of a Databricks service principal, from the workspace's
/// token endpoint (`/oidc/v1/token`, client credentials, scope
/// [`SCOPE`]).
///
/// [`token`](TokenSource::token) returns the cached token while it has more
/// than five minutes left, and fetches a new one after that. Clones share
/// the token.
#[derive(Clone)]
pub struct DatabricksOAuth {
    config: Arc<DatabricksConfig>,
    url: String,
    client_id: String,
}

impl std::fmt::Debug for DatabricksOAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabricksOAuth")
            .field("url", &self.url)
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

impl DatabricksOAuth {
    /// The OAuth client `client_id` of a service principal on the workspace
    /// `host` (`https://…`, or a bare host name, which means HTTPS).
    pub fn new(
        host: &str,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Result<Self> {
        Self::build(host, client_id.into(), client_secret.into(), None)
    }

    fn build(
        host: &str,
        client_id: String,
        client_secret: String,
        retry: Option<RetryConfig>,
    ) -> Result<Self> {
        let workspace = workspace_url(host);
        let mut builder = DatabricksBuilder::new()
            .with_host(&workspace)
            .with_auth_type("oauth-m2m")
            .with_config(DatabricksConfigKey::ClientId, &client_id)
            .with_config(DatabricksConfigKey::ClientSecret, client_secret)
            .with_config(DatabricksConfigKey::Scope, SCOPE)
            // Plain HTTP only for a local endpoint (tests): a workspace is HTTPS.
            .with_client_options(
                ClientOptions::new().with_allow_http(workspace.starts_with("http://")),
            );
        if let Some(retry) = retry {
            builder = builder.with_retry(retry);
        }
        let config = builder
            .build(None)
            .map_err(|e| Error::invalid(format!("a Databricks OAuth client: {e}")))?;
        Ok(Self {
            config: Arc::new(config),
            url: format!("{workspace}/oidc/v1/token"),
            client_id,
        })
    }

    /// The client Databricks Apps inject for the App's own service
    /// principal: `DATABRICKS_HOST`, `DATABRICKS_CLIENT_ID` and
    /// `DATABRICKS_CLIENT_SECRET`.
    pub fn from_env() -> Result<Self> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        match (
            var(HOST_ENV),
            var("DATABRICKS_CLIENT_ID"),
            var("DATABRICKS_CLIENT_SECRET"),
        ) {
            (Some(host), Some(id), Some(secret)) => Self::new(&host, id, secret),
            _ => Err(Error::invalid(
                "a Databricks OAuth client needs DATABRICKS_HOST, DATABRICKS_CLIENT_ID and \
                 DATABRICKS_CLIENT_SECRET",
            )),
        }
    }

    /// The token endpoint.
    pub fn token_url(&self) -> &str {
        &self.url
    }

    /// The service principal's OAuth client id (its application id).
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
}

#[async_trait]
impl TokenSource for DatabricksOAuth {
    async fn token(&self) -> Result<String> {
        let credential = self
            .config
            .credentials
            .get_credential()
            .await
            .map_err(|e| {
                Error::internal(format!(
                    "fetching a Databricks OAuth token from {}: {e}",
                    self.url
                ))
            })?;
        Ok(credential.bearer.clone())
    }
}

/// The workspace's base URL: `host` with `https://` when it has no scheme,
/// without a trailing `/`.
pub fn workspace_url(host: &str) -> String {
    let host = host.trim_end_matches('/');
    if host.starts_with("https://") || host.starts_with("http://") {
        host.to_owned()
    } else {
        format!("https://{host}")
    }
}

/// A client that doesn't retry a failed token request, so tests see the
/// failure at once.
#[cfg(test)]
pub(crate) fn without_retries(host: &str, id: &str, secret: &str) -> DatabricksOAuth {
    let retry = RetryConfig {
        max_retries: 0,
        ..Default::default()
    };
    DatabricksOAuth::build(host, id.into(), secret.into(), Some(retry)).unwrap()
}

/// A fake token endpoint, for this crate's tests.
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;

    /// What the fake endpoint answers, in order; past the end, the last.
    #[derive(Clone)]
    pub enum Reply {
        Token(&'static str, u64),
        Fail,
    }

    /// A request's `Authorization` header and form body.
    pub type Request = (Option<String>, String);

    #[derive(Clone, Default)]
    pub struct Endpoint {
        replies: Arc<Mutex<Vec<Reply>>>,
        pub requests: Arc<Mutex<Vec<Request>>>,
    }

    impl Endpoint {
        pub fn calls(&self) -> usize {
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

    /// Serve `replies` at `/oidc/v1/token`; returns the endpoint and the
    /// workspace's URL.
    pub async fn serve(replies: Vec<Reply>) -> (Endpoint, String) {
        let endpoint = Endpoint {
            replies: Arc::new(Mutex::new(replies)),
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
}

#[cfg(test)]
mod tests {
    use super::fake::{Reply, serve};
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_token_url_is_the_workspaces_oidc_endpoint() {
        for host in [
            "adb-1.azuredatabricks.net",
            "https://adb-1.azuredatabricks.net/",
        ] {
            assert_eq!(
                DatabricksOAuth::new(host, "id", "secret")
                    .unwrap()
                    .token_url(),
                "https://adb-1.azuredatabricks.net/oidc/v1/token"
            );
        }
        let debug = format!("{:?}", DatabricksOAuth::new("h", "id", "hunter2").unwrap());
        assert!(!debug.contains("hunter2"), "{debug}");
    }

    #[tokio::test]
    async fn fetches_with_client_credentials_and_caches_the_token() {
        let (endpoint, host) = serve(vec![Reply::Token("t1", 3600)]).await;
        let oauth = DatabricksOAuth::new(&host, "sp-id", "sp-secret").unwrap();
        assert_eq!(oauth.token().await.unwrap(), "t1");
        assert_eq!(oauth.clone().token().await.unwrap(), "t1");
        assert_eq!(endpoint.calls(), 1, "the second call uses the cached token");

        let (_, body) = endpoint.requests.lock().unwrap()[0].clone();
        for field in [
            "grant_type=client_credentials",
            "client_id=sp-id",
            "client_secret=sp-secret",
            "scope=all-apis",
        ] {
            assert!(body.contains(field), "{body}");
        }
    }

    #[tokio::test]
    async fn renews_a_token_within_five_minutes_of_expiry() {
        // Good for a minute: inside the margin, so the next call renews it.
        let (endpoint, host) = serve(vec![Reply::Token("t1", 60), Reply::Token("t2", 3600)]).await;
        let oauth = DatabricksOAuth::new(&host, "id", "secret").unwrap();
        assert_eq!(oauth.token().await.unwrap(), "t1");
        // olai-http holds a fresh token for 100 ms before re-fetching.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(oauth.token().await.unwrap(), "t2");
        assert_eq!(oauth.token().await.unwrap(), "t2");
        assert_eq!(endpoint.calls(), 2);
    }

    #[tokio::test]
    async fn a_failed_fetch_is_an_error_and_the_next_call_retries() {
        let (endpoint, host) = serve(vec![Reply::Fail, Reply::Token("t1", 3600)]).await;
        let oauth = without_retries(&host, "id", "secret");
        let err = oauth.token().await.unwrap_err();
        assert!(err.to_string().contains("OAuth token"), "{err}");
        assert_eq!(oauth.token().await.unwrap(), "t1");
        assert_eq!(endpoint.calls(), 2);
    }
}
