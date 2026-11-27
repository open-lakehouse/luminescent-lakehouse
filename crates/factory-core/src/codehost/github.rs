//! [`CodeHost`] for GitHub, over its GraphQL API: one query per PR, polled.
//!
//! The token comes from [`GitHubAuth`]: given; the `gh` CLI's (`gh auth
//! token`), read once and read again when GitHub refuses it; or a GitHub
//! App's installation token for the repo's owner ([`GitHubApp`]), minted
//! when needed and renewed before it expires.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{CodeHost, PrStatus};
use crate::{Error, Result};

/// Where the GitHub token comes from.
#[derive(Clone)]
pub enum GitHubAuth {
    Token(String),
    /// The `gh` CLI's token for `hostname` (`github.com`).
    GhCli {
        hostname: String,
    },
    /// A GitHub App's installation tokens, one per repo owner.
    App(Arc<GitHubApp>),
}

impl std::fmt::Debug for GitHubAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Token(_) => f.write_str("Token(..)"),
            Self::GhCli { hostname } => {
                f.debug_struct("GhCli").field("hostname", hostname).finish()
            }
            Self::App(app) => app.fmt(f),
        }
    }
}

impl GitHubAuth {
    /// The `gh` CLI's token for github.com.
    pub fn gh() -> Self {
        Self::GhCli {
            hostname: "github.com".into(),
        }
    }

    /// The token for the repo `owner/name`; `fresh` when GitHub refused the
    /// last one.
    async fn read(&self, owner: &str, name: &str, fresh: bool) -> Result<String> {
        match self {
            Self::Token(t) => Ok(t.clone()),
            Self::App(app) => app.token(owner, name, fresh).await,
            Self::GhCli { hostname } => {
                let out = tokio::process::Command::new("gh")
                    .args(["auth", "token", "--hostname", hostname])
                    .output()
                    .await
                    .map_err(|e| Error::internal(format!("running gh auth token: {e}")))?;
                if !out.status.success() {
                    return Err(Error::internal(format!(
                        "gh auth token failed: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    )));
                }
                let token = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                if token.is_empty() {
                    return Err(Error::internal("gh auth token printed no token"));
                }
                Ok(token)
            }
        }
    }
}

/// A GitHub App: JWTs from its private key, and installation tokens for the
/// owners it's installed on, cached per owner and renewed
/// [`MARGIN`](Self::MARGIN) before they expire (they last an hour).
pub struct GitHubApp {
    app_id: String,
    key: RsaKeyPair,
    /// The REST API's root.
    api: String,
    http: reqwest::Client,
    /// Owner (lower case) → its installation token.
    tokens: tokio::sync::Mutex<HashMap<String, Installation>>,
}

#[derive(Clone)]
struct Installation {
    id: u64,
    token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    renew_at: Instant,
}

/// An installation token, as [`GitHubApp::mint_for_owner`] mints it.
#[derive(Clone)]
pub struct InstallationToken {
    /// The owner (user or organization) the installation is on, as GitHub
    /// spells it.
    pub owner: String,
    pub token: String,
    /// When GitHub stops accepting it (an hour after it's minted).
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl std::fmt::Debug for InstallationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstallationToken")
            .field("owner", &self.owner)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for GitHubApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubApp")
            .field("app_id", &self.app_id)
            .field("api", &self.api)
            .finish_non_exhaustive()
    }
}

impl GitHubApp {
    /// How long before an installation token expires it's renewed.
    pub const MARGIN: Duration = Duration::from_secs(5 * 60);

    /// The App `app_id` (or its client id), signing with `private_key`, the
    /// PEM GitHub issues (PKCS#1, `BEGIN RSA PRIVATE KEY`; PKCS#8 works
    /// too).
    pub fn new(app_id: impl Into<String>, private_key: &str) -> Result<Self> {
        let app_id = app_id.into();
        if app_id.trim().is_empty() {
            return Err(Error::invalid("a GitHub App needs its app id"));
        }
        Ok(Self {
            app_id,
            key: parse_key(private_key)?,
            api: "https://api.github.com".into(),
            http: reqwest::Client::new(),
            tokens: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    /// Another REST API root (GitHub Enterprise's `https://<host>/api/v3`, a
    /// test server).
    #[must_use]
    pub fn with_api(mut self, api: impl Into<String>) -> Self {
        self.api = api.into().trim_end_matches('/').to_owned();
        self
    }

    /// The App's JWT: issued a minute ago (for clock drift), good for nine
    /// more, as GitHub allows ten at most.
    pub fn jwt(&self) -> Result<String> {
        let now = chrono::Utc::now().timestamp();
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = json!({"iat": now - 60, "exp": now + 9 * 60, "iss": self.app_id});
        let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
        let message = format!("{header}.{claims}");
        let mut signature = vec![0; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &ring::rand::SystemRandom::new(),
                message.as_bytes(),
                &mut signature,
            )
            .map_err(|_| Error::internal("signing the GitHub App's JWT"))?;
        Ok(format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }

    /// The installation token for `owner` (found through its repo `name`):
    /// the cached one while it's current, a new one once it's due or when
    /// `fresh`.
    pub async fn token(&self, owner: &str, name: &str, fresh: bool) -> Result<String> {
        let mut tokens = self.tokens.lock().await;
        let cached = tokens.get(&owner.to_lowercase()).cloned();
        if let Some(t) = &cached
            && !fresh
            && Instant::now() < t.renew_at
        {
            return Ok(t.token.clone());
        }
        let id = match cached {
            Some(t) => t.id,
            None => self.installation(owner, name).await?,
        };
        let minted = match self.mint(id).await {
            Err(_) if fresh => {
                // The App may have been reinstalled: look the id up again.
                let id = self.installation(owner, name).await?;
                self.mint(id).await?
            }
            other => other?,
        };
        let token = minted.token.clone();
        tokens.insert(owner.to_lowercase(), minted);
        Ok(token)
    }

    /// A new installation token for `owner`, or for the App's only
    /// installation when `owner` is `None` (an error when it has none or
    /// several). The installation is found through the App's installations
    /// (`GET /app/installations`) and remembered; `relookup` (after a token
    /// was refused, as when the App was reinstalled) finds it again. The
    /// token replaces the cached one, so [`token`](Self::token) hands it out
    /// too.
    pub async fn mint_for_owner(
        &self,
        owner: Option<&str>,
        relookup: bool,
    ) -> Result<InstallationToken> {
        let mut tokens = self.tokens.lock().await;
        let cached = match owner {
            Some(owner) if !relookup => tokens
                .get(&owner.to_lowercase())
                .map(|t| (owner.to_owned(), t.id)),
            _ => None,
        };
        let (login, id) = match cached {
            Some(found) => found,
            None => self.installation_of(owner).await?,
        };
        let minted = self.mint(id).await?;
        let token = InstallationToken {
            owner: login.clone(),
            token: minted.token.clone(),
            expires_at: minted.expires_at,
        };
        tokens.insert(login.to_lowercase(), minted);
        Ok(token)
    }

    /// The App's installations: each one's owner (its account's login) and id.
    pub async fn installations(&self) -> Result<Vec<(String, u64)>> {
        let mut all = Vec::new();
        for page in 1.. {
            let url = format!("{}/app/installations?per_page=100&page={page}", self.api);
            let body = self.call(self.http.get(&url)).await?;
            let list = body
                .as_array()
                .ok_or_else(|| Error::internal(format!("github app: {url} answered no list")))?;
            all.extend(list.iter().filter_map(|i| {
                let login = i.get("account")?.get("login")?.as_str()?;
                Some((login.to_owned(), i.get("id")?.as_u64()?))
            }));
            if list.len() < 100 {
                break;
            }
        }
        Ok(all)
    }

    /// The installation on `owner` (any case), or the only one there is.
    async fn installation_of(&self, owner: Option<&str>) -> Result<(String, u64)> {
        let all = self.installations().await?;
        let names = || all.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>();
        match owner {
            Some(owner) => all
                .iter()
                .find(|(login, _)| login.eq_ignore_ascii_case(owner))
                .cloned()
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "the GitHub App isn't installed on {owner} (it is on {:?})",
                        names()
                    ))
                }),
            None => match all.as_slice() {
                [one] => Ok(one.clone()),
                [] => Err(Error::invalid("the GitHub App has no installations")),
                _ => Err(Error::invalid(format!(
                    "the GitHub App is installed on several owners ({:?}): name one",
                    names()
                ))),
            },
        }
    }

    /// The id of the App's installation on `owner/name`.
    async fn installation(&self, owner: &str, name: &str) -> Result<u64> {
        let url = format!("{}/repos/{owner}/{name}/installation", self.api);
        let body = self.call(self.http.get(&url)).await?;
        body.get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::internal(format!("github app: {url} answered without an id")))
    }

    /// A new token for the installation `id`.
    async fn mint(&self, id: u64) -> Result<Installation> {
        let url = format!("{}/app/installations/{id}/access_tokens", self.api);
        let body = self.call(self.http.post(&url)).await?;
        let token = body
            .get("token")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::internal("github app: an access token without a token"))?;
        let expires_at = body
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);
        let left = (expires_at - chrono::Utc::now())
            .to_std()
            .unwrap_or_default();
        Ok(Installation {
            id,
            token: token.to_owned(),
            expires_at,
            renew_at: Instant::now() + left.saturating_sub(Self::MARGIN),
        })
    }

    /// `request` as the App, and its JSON body, or why GitHub refused it.
    async fn call(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let response = request
            .bearer_auth(self.jwt()?)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header(reqwest::header::USER_AGENT, "factory-core")
            .send()
            .await
            .map_err(|e| Error::internal(format!("github app: {e}")))?;
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(Error::internal(format!(
                "github app: {status}: {}",
                s(&body, "message")
            )));
        }
        Ok(body)
    }
}

/// An RSA private key from its PEM: PKCS#1 or PKCS#8.
fn parse_key(pem: &str) -> Result<RsaKeyPair> {
    let invalid = |why: &str| Error::invalid(format!("the GitHub App's private key: {why}"));
    let pem = pem.trim();
    let (label, rest) = pem
        .strip_prefix("-----BEGIN ")
        .and_then(|r| r.split_once("-----"))
        .ok_or_else(|| invalid("not a PEM"))?;
    let body = rest
        .split_once(&format!("-----END {label}-----"))
        .map(|(body, _)| body)
        .ok_or_else(|| invalid("no END line"))?;
    let der = STANDARD
        .decode(body.split_whitespace().collect::<String>())
        .map_err(|_| invalid("not base64"))?;
    match label {
        "RSA PRIVATE KEY" => RsaKeyPair::from_der(&der),
        "PRIVATE KEY" => RsaKeyPair::from_pkcs8(&der),
        other => return Err(invalid(&format!("a {other}, not an RSA private key"))),
    }
    .map_err(|e| invalid(&e.to_string()))
}

const QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      url title state merged isDraft reviewDecision headRefName baseRefName
      commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
    }
  }
}";

/// GitHub's PRs.
pub struct GitHub {
    http: reqwest::Client,
    /// The GraphQL endpoint.
    api: String,
    /// The web host PR URLs are on.
    web_host: String,
    auth: GitHubAuth,
    token: Mutex<Option<String>>,
}

impl GitHub {
    /// GitHub.com with `auth`.
    pub fn new(auth: GitHubAuth) -> Self {
        Self {
            http: reqwest::Client::new(),
            api: "https://api.github.com/graphql".into(),
            web_host: "github.com".into(),
            auth,
            token: Mutex::new(None),
        }
    }

    /// Another GraphQL endpoint (GitHub Enterprise, a test server).
    #[must_use]
    pub fn with_api(mut self, api: impl Into<String>, web_host: impl Into<String>) -> Self {
        self.api = api.into();
        self.web_host = web_host.into();
        self
    }

    async fn token(&self, owner: &str, name: &str, fresh: bool) -> Result<String> {
        if let GitHubAuth::App(app) = &self.auth {
            // Per owner; the App caches.
            return app.token(owner, name, fresh).await;
        }
        if !fresh && let Some(t) = self.token.lock().expect("token lock").clone() {
            return Ok(t);
        }
        let t = self.auth.read(owner, name, fresh).await?;
        *self.token.lock().expect("token lock") = Some(t.clone());
        Ok(t)
    }

    async fn query(
        &self,
        owner: &str,
        name: &str,
        number: i64,
        fresh: bool,
    ) -> Result<(reqwest::StatusCode, Value)> {
        let vars = json!({ "owner": owner, "name": name, "number": number });
        let response = self
            .http
            .post(&self.api)
            .bearer_auth(self.token(owner, name, fresh).await?)
            .header(reqwest::header::USER_AGENT, "factory-core")
            .json(&json!({ "query": QUERY, "variables": vars }))
            .send()
            .await
            .map_err(|e| Error::internal(format!("github: {e}")))?;
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        Ok((status, body))
    }
}

/// `https://<host>/<owner>/<name>/pull/<n>` → (owner, name, n).
pub fn parse_url(url: &str, host: &str) -> Option<(String, String, i64)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?
        .strip_prefix(host)?
        .strip_prefix('/')?;
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    match parts.as_slice() {
        [owner, name, "pull", n, ..] => Some(((*owner).into(), (*name).into(), n.parse().ok()?)),
        _ => None,
    }
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// GitHub's PR object in the factory's vocabulary.
pub fn status(pr: &Value) -> PrStatus {
    let state = if pr.get("merged").and_then(Value::as_bool) == Some(true) {
        "merged"
    } else {
        match s(pr, "state") {
            "MERGED" => "merged",
            "CLOSED" => "closed",
            _ => "open",
        }
    };
    let rollup = pr
        .pointer("/commits/nodes/0/commit/statusCheckRollup/state")
        .and_then(Value::as_str)
        .unwrap_or_default();
    PrStatus {
        url: s(pr, "url").into(),
        title: s(pr, "title").into(),
        head: s(pr, "headRefName").into(),
        base: s(pr, "baseRefName").into(),
        state: state.into(),
        draft: pr
            .get("isDraft")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
        review_decision: s(pr, "reviewDecision").to_lowercase(),
        checks: match rollup {
            "SUCCESS" => "passing",
            "FAILURE" | "ERROR" => "failing",
            "PENDING" | "EXPECTED" => "pending",
            _ => "",
        }
        .into(),
    }
}

#[async_trait]
impl CodeHost for GitHub {
    fn handles(&self, url: &str) -> bool {
        parse_url(url, &self.web_host).is_some()
    }

    async fn pull_request(&self, url: &str) -> Result<PrStatus> {
        let (owner, name, number) = parse_url(url, &self.web_host)
            .ok_or_else(|| Error::invalid(format!("{url} isn't a GitHub PR URL")))?;
        let (mut code, mut body) = self.query(&owner, &name, number, false).await?;
        if code == reqwest::StatusCode::UNAUTHORIZED
            && matches!(self.auth, GitHubAuth::GhCli { .. } | GitHubAuth::App(_))
        {
            (code, body) = self.query(&owner, &name, number, true).await?;
        }
        if !code.is_success() {
            return Err(Error::internal(format!(
                "github: {code}: {}",
                s(&body, "message")
            )));
        }
        if let Some(errors) = body.get("errors").and_then(Value::as_array)
            && let Some(first) = errors.first()
        {
            return Err(Error::internal(format!("github: {}", s(first, "message"))));
        }
        let pr = body
            .pointer("/data/repository/pullRequest")
            .filter(|v| !v.is_null())
            .ok_or_else(|| Error::NotFound(format!("pull request {url}")))?;
        let mut out = status(pr);
        if out.url.is_empty() {
            out.url = url.into();
        }
        Ok(out)
    }
}

/// A throwaway GitHub App key, for tests here and in other crates.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;

    /// A PEM block of `der` under `label`.
    pub fn pem(label: &str, der: &[u8]) -> String {
        let body = STANDARD.encode(der);
        let lines: Vec<&str> = body
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        let dashes = "-----";
        format!(
            "{dashes}BEGIN {label}{dashes}\n{}\n{dashes}END {label}{dashes}\n",
            lines.join("\n")
        )
    }

    /// A throwaway RSA key, generated once per test run, as GitHub issues
    /// them (PKCS#1 PEM).
    pub fn test_key() -> &'static str {
        static KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        KEY.get_or_init(|| {
            use rsa::pkcs1::EncodeRsaPrivateKey as _;
            let key = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
            pem("RSA PRIVATE KEY", key.to_pkcs1_der().unwrap().as_bytes())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn urls() {
        assert_eq!(
            parse_url("https://github.com/o/r/pull/12", "github.com"),
            Some(("o".into(), "r".into(), 12))
        );
        assert_eq!(
            parse_url("https://github.com/o/r/pull/12/files", "github.com"),
            Some(("o".into(), "r".into(), 12))
        );
        assert!(parse_url("https://github.com/o/r/issues/12", "github.com").is_none());
        assert!(parse_url("https://gitlab.com/o/r/pull/12", "github.com").is_none());
    }

    #[test]
    fn statuses() {
        let pr = json!({
            "url": "u", "title": "t", "state": "OPEN", "merged": false, "isDraft": true,
            "reviewDecision": "CHANGES_REQUESTED", "headRefName": "h", "baseRefName": "main",
            "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "FAILURE"}}}]}
        });
        let got = status(&pr);
        assert_eq!(
            (
                got.state.as_str(),
                got.draft,
                got.review_decision.as_str(),
                got.checks.as_str()
            ),
            ("open", true, "changes_requested", "failing")
        );
        let merged = json!({"state": "MERGED", "merged": true, "commits": {"nodes": []}});
        let got = status(&merged);
        assert_eq!((got.state.as_str(), got.checks.as_str()), ("merged", ""));
        assert_eq!(status(&json!({"state": "CLOSED"})).state, "closed");
    }

    /// A GraphQL endpoint that refuses the first token and answers with one
    /// merged PR.
    async fn serve(refuse_first: bool) -> (String, Arc<AtomicUsize>) {
        use axum::Json;
        use axum::http::{HeaderMap, StatusCode};
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let app = axum::Router::new().route(
            "/graphql",
            axum::routing::post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let seen = seen.clone();
                async move {
                    let n = seen.fetch_add(1, Ordering::SeqCst);
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    assert_eq!(body["variables"]["number"], 7);
                    if refuse_first && n == 0 {
                        return (StatusCode::UNAUTHORIZED, Json(json!({"message": "Bad credentials"})));
                    }
                    assert_eq!(auth, "Bearer t0k");
                    (
                        StatusCode::OK,
                        Json(json!({"data": {"repository": {"pullRequest": {
                            "url": "https://github.example/o/r/pull/7", "state": "MERGED", "merged": true,
                            "headRefName": "h", "baseRefName": "main", "reviewDecision": "APPROVED",
                            "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]}
                        }}}})),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/graphql"), calls)
    }

    #[tokio::test]
    async fn reads_a_pr() {
        let (api, calls) = serve(false).await;
        let gh = GitHub::new(GitHubAuth::Token("t0k".into())).with_api(api, "github.example");
        let url = "https://github.example/o/r/pull/7";
        assert!(gh.handles(url) && !gh.handles("https://github.com/o/r/pull/7"));
        let pr = gh.pull_request(url).await.unwrap();
        assert!(pr.merged());
        assert_eq!(
            (pr.checks.as_str(), pr.review_decision.as_str()),
            ("passing", "approved")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_given_token_isnt_retried() {
        let (api, calls) = serve(true).await;
        let gh = GitHub::new(GitHubAuth::Token("t0k".into())).with_api(api, "github.example");
        let err = gh
            .pull_request("https://github.example/o/r/pull/7")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("401"), "{err}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    use super::testing::{pem, test_key};

    /// What the fake GitHub saw and answers: installations by owner, the
    /// tokens it mints, and the GraphQL bearer it last saw.
    #[derive(Default)]
    struct Fake {
        mints: AtomicUsize,
        lookups: AtomicUsize,
        /// Seconds each minted token lasts.
        lasts: std::sync::atomic::AtomicI64,
        /// Refuse the GraphQL call once with a 401.
        refuse_once: std::sync::atomic::AtomicBool,
        bearers: Mutex<Vec<String>>,
        jwts: Mutex<Vec<String>>,
        /// The App's installations, as `GET /app/installations` lists them.
        installed: Mutex<Vec<(&'static str, u64)>>,
        listings: AtomicUsize,
    }

    async fn serve_app(fake: Arc<Fake>) -> String {
        use axum::extract::{Path, State};
        use axum::http::{HeaderMap, StatusCode};
        use axum::routing::{get, post};
        fn bearer(h: &HeaderMap) -> String {
            h.get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .unwrap_or_default()
                .to_owned()
        }
        let app = axum::Router::new()
            .route(
                "/app/installations",
                get(|State(f): State<Arc<Fake>>, h: HeaderMap| async move {
                    f.listings.fetch_add(1, Ordering::SeqCst);
                    f.jwts.lock().unwrap().push(bearer(&h));
                    let list: Vec<Value> = f
                        .installed
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(login, id)| json!({"id": id, "account": {"login": login}}))
                        .collect();
                    axum::Json(Value::Array(list))
                }),
            )
            .route(
                "/repos/{owner}/{name}/installation",
                get(
                    |State(f): State<Arc<Fake>>,
                     Path((owner, _)): Path<(String, String)>,
                     h: HeaderMap| async move {
                        f.lookups.fetch_add(1, Ordering::SeqCst);
                        f.jwts.lock().unwrap().push(bearer(&h));
                        let id = if owner == "o" { 1 } else { 2 };
                        axum::Json(json!({ "id": id }))
                    },
                ),
            )
            .route(
                "/app/installations/{id}/access_tokens",
                post(
                    |State(f): State<Arc<Fake>>, Path(id): Path<u64>, h: HeaderMap| async move {
                        let n = f.mints.fetch_add(1, Ordering::SeqCst) + 1;
                        f.jwts.lock().unwrap().push(bearer(&h));
                        let lasts = f.lasts.load(Ordering::SeqCst);
                        let at = chrono::Utc::now() + chrono::Duration::seconds(lasts);
                        axum::Json(json!({
                            "token": format!("ghs_{id}_{n}"),
                            "expires_at": at.to_rfc3339(),
                        }))
                    },
                ),
            )
            .route(
                "/graphql",
                post(|State(f): State<Arc<Fake>>, h: HeaderMap| async move {
                    f.bearers.lock().unwrap().push(bearer(&h));
                    if f.refuse_once.swap(false, Ordering::SeqCst) {
                        return (
                            StatusCode::UNAUTHORIZED,
                            axum::Json(json!({"message": "Bad credentials"})),
                        );
                    }
                    (
                        StatusCode::OK,
                        axum::Json(json!({"data": {"repository": {"pullRequest": {
                            "url": "", "state": "OPEN", "merged": false, "commits": {"nodes": []}
                        }}}})),
                    )
                }),
            )
            .with_state(fake);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn app_host(api: &str) -> GitHub {
        let app = GitHubApp::new("12345", test_key()).unwrap().with_api(api);
        GitHub::new(GitHubAuth::App(Arc::new(app)))
            .with_api(format!("{api}/graphql"), "github.example")
    }

    #[test]
    fn the_apps_jwt_is_signed_with_its_key() {
        use ring::signature::{KeyPair as _, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
        let app = GitHubApp::new("12345", test_key()).unwrap();
        let jwt = app.jwt().unwrap();
        let (message, signature) = jwt.rsplit_once('.').unwrap();
        let public = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, app.key.public_key());
        public
            .verify(
                message.as_bytes(),
                &URL_SAFE_NO_PAD.decode(signature).unwrap(),
            )
            .expect("a valid RS256 signature");
        let claims: Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(message.split_once('.').unwrap().1)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(claims["iss"], "12345");
        let (iat, exp) = (
            claims["iat"].as_i64().unwrap(),
            claims["exp"].as_i64().unwrap(),
        );
        assert_eq!(exp - iat, 10 * 60, "ten minutes, GitHub's most");

        assert!(GitHubApp::new("", test_key()).is_err());
        let err = GitHubApp::new("1", "nope").unwrap_err().to_string();
        assert!(err.contains("not a PEM"), "{err}");
        let other = pem("EC PRIVATE KEY", &[0; 4]);
        let err = GitHubApp::new("1", &other).unwrap_err().to_string();
        assert!(err.contains("not an RSA private key"), "{err}");
        assert!(!format!("{app:?}").contains("PRIVATE"));
        // PKCS#8 works too.
        use rsa::pkcs1::DecodeRsaPrivateKey as _;
        use rsa::pkcs8::EncodePrivateKey as _;
        let der = STANDARD
            .decode(
                test_key()
                    .lines()
                    .filter(|l| !l.starts_with('-'))
                    .collect::<String>(),
            )
            .unwrap();
        let pkcs8 = rsa::RsaPrivateKey::from_pkcs1_der(&der)
            .unwrap()
            .to_pkcs8_der()
            .unwrap();
        assert!(GitHubApp::new("1", &pem("PRIVATE KEY", pkcs8.as_bytes())).is_ok());
    }

    #[tokio::test]
    async fn an_app_mints_a_token_per_owner_and_caches_it() {
        let fake = Arc::new(Fake::default());
        fake.lasts.store(3600, Ordering::SeqCst);
        let gh = app_host(&serve_app(fake.clone()).await);
        gh.pull_request("https://github.example/o/r/pull/1")
            .await
            .unwrap();
        gh.pull_request("https://github.example/O/other/pull/2")
            .await
            .unwrap();
        gh.pull_request("https://github.example/p/r/pull/3")
            .await
            .unwrap();
        assert_eq!(fake.mints.load(Ordering::SeqCst), 2, "one per owner");
        assert_eq!(fake.lookups.load(Ordering::SeqCst), 2);
        assert_eq!(
            *fake.bearers.lock().unwrap(),
            ["ghs_1_1", "ghs_1_1", "ghs_2_2"]
        );
        // The App's own calls carry its JWT, never an installation token.
        assert!(
            fake.jwts
                .lock()
                .unwrap()
                .iter()
                .all(|j| j.split('.').count() == 3)
        );
    }

    #[tokio::test]
    async fn an_app_renews_a_token_before_it_expires() {
        let fake = Arc::new(Fake::default());
        // Inside the renewal margin: due at once.
        fake.lasts.store(60, Ordering::SeqCst);
        let gh = app_host(&serve_app(fake.clone()).await);
        gh.pull_request("https://github.example/o/r/pull/1")
            .await
            .unwrap();
        gh.pull_request("https://github.example/o/r/pull/1")
            .await
            .unwrap();
        assert_eq!(fake.mints.load(Ordering::SeqCst), 2);
        assert_eq!(
            fake.lookups.load(Ordering::SeqCst),
            1,
            "the installation is remembered"
        );
        assert_eq!(*fake.bearers.lock().unwrap(), ["ghs_1_1", "ghs_1_2"]);
    }

    #[tokio::test]
    async fn an_app_mints_again_when_github_refuses_its_token() {
        let fake = Arc::new(Fake::default());
        fake.lasts.store(3600, Ordering::SeqCst);
        fake.refuse_once.store(true, Ordering::SeqCst);
        let gh = app_host(&serve_app(fake.clone()).await);
        gh.pull_request("https://github.example/o/r/pull/1")
            .await
            .unwrap();
        assert_eq!(*fake.bearers.lock().unwrap(), ["ghs_1_1", "ghs_1_2"]);
    }

    #[tokio::test]
    async fn an_app_mints_for_an_owner_and_shares_the_cache() {
        let fake = Arc::new(Fake::default());
        fake.lasts.store(3600, Ordering::SeqCst);
        *fake.installed.lock().unwrap() = vec![("Open-Lakehouse", 1)];
        let api = serve_app(fake.clone()).await;
        let app = Arc::new(GitHubApp::new("12345", test_key()).unwrap().with_api(&api));

        // The only installation, without naming it; its login as GitHub spells it.
        let first = app.mint_for_owner(None, false).await.unwrap();
        assert_eq!(
            (first.owner.as_str(), first.token.as_str()),
            ("Open-Lakehouse", "ghs_1_1")
        );
        let left = first.expires_at - chrono::Utc::now();
        assert!(left > chrono::Duration::minutes(59), "{left}");
        assert!(!format!("{first:?}").contains("ghs_"));

        // Named in any case, it's remembered: a new token, no new listing.
        let second = app
            .mint_for_owner(Some("open-lakehouse"), false)
            .await
            .unwrap();
        assert_eq!(second.token, "ghs_1_2");
        assert_eq!(fake.listings.load(Ordering::SeqCst), 1);
        // The code host hands out the newest token.
        assert_eq!(
            app.token("open-lakehouse", "r", false).await.unwrap(),
            "ghs_1_2"
        );
        assert_eq!(fake.lookups.load(Ordering::SeqCst), 0);

        // A refused token looks the installation up again.
        app.mint_for_owner(Some("open-lakehouse"), true)
            .await
            .unwrap();
        assert_eq!(fake.listings.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_app_needs_an_owner_named_unless_it_has_one_installation() {
        let fake = Arc::new(Fake::default());
        fake.lasts.store(3600, Ordering::SeqCst);
        let api = serve_app(fake.clone()).await;
        let app = GitHubApp::new("12345", test_key()).unwrap().with_api(&api);

        let err = app
            .mint_for_owner(None, false)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no installations"), "{err}");
        *fake.installed.lock().unwrap() = vec![("o", 1), ("p", 2)];
        let err = app
            .mint_for_owner(None, false)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("several owners") && err.contains("\"p\""),
            "{err}"
        );
        let err = app
            .mint_for_owner(Some("q"), false)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("isn't installed on q"), "{err}");
        let p = app.mint_for_owner(Some("P"), false).await.unwrap();
        assert_eq!((p.owner.as_str(), p.token.as_str()), ("p", "ghs_2_1"));
        assert_eq!(fake.mints.load(Ordering::SeqCst), 1);
    }
}
