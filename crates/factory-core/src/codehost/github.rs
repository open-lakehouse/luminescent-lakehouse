//! [`CodeHost`] for GitHub, over its GraphQL API: one query per PR, polled.
//!
//! The token comes from [`GitHubAuth`]: given, or the `gh` CLI's
//! (`gh auth token`), read once and read again when GitHub refuses it.

use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{CodeHost, PrStatus};
use crate::{Error, Result};

/// Where the GitHub token comes from.
#[derive(Clone, PartialEq, Eq)]
pub enum GitHubAuth {
    Token(String),
    /// The `gh` CLI's token for `hostname` (`github.com`).
    GhCli {
        hostname: String,
    },
}

impl std::fmt::Debug for GitHubAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Token(_) => f.write_str("Token(..)"),
            Self::GhCli { hostname } => {
                f.debug_struct("GhCli").field("hostname", hostname).finish()
            }
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

    async fn read(&self) -> Result<String> {
        match self {
            Self::Token(t) => Ok(t.clone()),
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

    async fn token(&self, fresh: bool) -> Result<String> {
        if !fresh && let Some(t) = self.token.lock().expect("token lock").clone() {
            return Ok(t);
        }
        let t = self.auth.read().await?;
        *self.token.lock().expect("token lock") = Some(t.clone());
        Ok(t)
    }

    async fn query(&self, vars: &Value, fresh: bool) -> Result<(reqwest::StatusCode, Value)> {
        let response = self
            .http
            .post(&self.api)
            .bearer_auth(self.token(fresh).await?)
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
        let vars = json!({ "owner": owner, "name": name, "number": number });
        let (mut code, mut body) = self.query(&vars, false).await?;
        if code == reqwest::StatusCode::UNAUTHORIZED
            && matches!(self.auth, GitHubAuth::GhCli { .. })
        {
            (code, body) = self.query(&vars, true).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
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
}
