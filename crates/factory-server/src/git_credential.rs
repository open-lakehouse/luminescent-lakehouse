//! [`GitCredentialRefresher`]: keeps the factory's Databricks Git credential
//! holding a current installation token of its GitHub App.
//!
//! A managed sandbox on hosted Omnigent gets GitHub from its creator's
//! Databricks Git credential: the sandbox's git helper reads it from the
//! workspace (`/api/2.0/git-credentials`), and `gh` is logged in with it. The
//! factory's sessions are its service principal's, so that principal holds a
//! `gitHub` credential with an installation token of the factory's App. The
//! token lasts an hour, so this task mints a new one
//! ([`GitHubApp::mint_for_owner`]) and writes it before the old one expires.
//! docs/factory/hosts.md has the design.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use factory_core::codehost::github::{GitHubApp, InstallationToken};
use factory_core::token::TokenSource;
use factory_core::{Error, Result};
use serde_json::{Value, json};

/// The credentials API's path under the workspace.
const PATH: &str = "/api/2.0/git-credentials";

/// Keeps one Databricks Git credential current: the default `gitHub`
/// credential of the principal whose token [`databricks`](Self::new) gives,
/// named `factory-github-app:<owner>`, holding an installation token of the
/// App on `owner` (user `x-access-token`).
///
/// Each round ([`refresh`](Self::refresh)) mints a new token and writes it:
///
/// - It updates the credential it wrote last. Otherwise it lists the
///   principal's credentials and updates the one with its name, or else the
///   principal's default `gitHub` credential (one set up by hand), and creates
///   one only when there's neither.
/// - An update answered 404 (the credential was deleted) lists and creates
///   again. Any other refusal (4xx but 429) is taken as a refused token: the
///   installation is looked up again, a new token minted, and written once
///   more.
///
/// [`spawn`](Self::spawn) runs the rounds: the next one
/// [`margin`](Self::with_margin) before the written token expires, and after a
/// failure, again after a backoff that doubles up to a minute.
pub struct GitCredentialRefresher {
    app: Arc<GitHubApp>,
    owner: Option<String>,
    workspace: String,
    databricks: Arc<dyn TokenSource>,
    http: reqwest::Client,
    margin: Duration,
    backoff: (Duration, Duration),
    /// The credential written last.
    credential_id: Option<u64>,
}

impl std::fmt::Debug for GitCredentialRefresher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitCredentialRefresher")
            .field("owner", &self.owner)
            .field("workspace", &self.workspace)
            .field("credential_id", &self.credential_id)
            .finish_non_exhaustive()
    }
}

/// How a write went.
enum Write {
    Done(u64),
    /// 404: the credential is gone.
    Missing,
    /// Refused (4xx but 404 and 429), with Databricks' message.
    Refused(String),
}

impl GitCredentialRefresher {
    /// How long before the written token expires the next one is written.
    pub const MARGIN: Duration = Duration::from_secs(10 * 60);
    /// The first retry's wait after a failed round, and the longest.
    pub const BACKOFF: (Duration, Duration) = (Duration::from_secs(5), Duration::from_secs(60));

    /// The credential of the principal `databricks` is a token of, on the
    /// workspace `workspace` (its base URL), for the App's installation on
    /// `owner` (the only one when `None`).
    pub fn new(
        app: Arc<GitHubApp>,
        owner: Option<String>,
        workspace: impl Into<String>,
        databricks: Arc<dyn TokenSource>,
    ) -> Self {
        Self {
            app,
            owner,
            workspace: workspace.into().trim_end_matches('/').to_owned(),
            databricks,
            http: reqwest::Client::new(),
            margin: Self::MARGIN,
            backoff: Self::BACKOFF,
            credential_id: None,
        }
    }

    /// Write the next token `margin` before the current one expires.
    #[must_use]
    pub fn with_margin(mut self, margin: Duration) -> Self {
        self.margin = margin;
        self
    }

    /// Retry a failed round after `first`, doubling up to `max`.
    #[must_use]
    pub fn with_backoff(mut self, first: Duration, max: Duration) -> Self {
        self.backoff = (first, max.max(first));
        self
    }

    /// The credential's name for `owner`.
    pub fn name(owner: &str) -> String {
        format!("factory-github-app:{owner}")
    }

    /// One round: mint a token and write it. Returns when it expires.
    pub async fn refresh(&mut self) -> Result<DateTime<Utc>> {
        let mut minted = self
            .app
            .mint_for_owner(self.owner.as_deref(), false)
            .await?;
        // The owner as GitHub spells it (the only installation, when none was
        // named), so the credential's name stays the same every round.
        self.owner = Some(minted.owner.clone());
        let mut reminted = false;
        loop {
            match self.write(&minted).await? {
                Ok(id) => {
                    self.credential_id = Some(id);
                    return Ok(minted.expires_at);
                }
                Err(why) if !reminted => {
                    tracing::warn!(owner = %minted.owner, %why, "the git credential refused the token: minting another");
                    minted = self.app.mint_for_owner(self.owner.as_deref(), true).await?;
                    reminted = true;
                }
                Err(why) => {
                    return Err(Error::internal(format!(
                        "the git credential refused a newly minted token too: {why}"
                    )));
                }
            }
        }
    }

    /// Write `token` into the credential, finding or creating it first.
    /// `Ok(Err(why))` when Databricks refused the token.
    async fn write(
        &mut self,
        token: &InstallationToken,
    ) -> Result<std::result::Result<u64, String>> {
        if let Some(id) = self.credential_id {
            match self.update(id, token).await? {
                Write::Done(id) => return Ok(Ok(id)),
                Write::Refused(why) => return Ok(Err(why)),
                Write::Missing => {
                    tracing::info!(id, "the git credential is gone: finding or creating it");
                    self.credential_id = None;
                }
            }
        }
        match self.find(&token.owner).await? {
            Some(id) => match self.update(id, token).await? {
                Write::Done(id) => Ok(Ok(id)),
                Write::Refused(why) => Ok(Err(why)),
                Write::Missing => Err(Error::internal(format!(
                    "git credential {id} was listed, then not found"
                ))),
            },
            None => match self.create(token).await? {
                Write::Done(id) => {
                    tracing::info!(id, owner = %token.owner, "created the git credential");
                    Ok(Ok(id))
                }
                Write::Refused(why) => Ok(Err(why)),
                Write::Missing => Err(Error::internal(format!("{PATH} answered 404"))),
            },
        }
    }

    /// The credential to update: ours by name, else the principal's default
    /// `gitHub` one.
    async fn find(&self, owner: &str) -> Result<Option<u64>> {
        let url = format!("{}{PATH}", self.workspace);
        let response = self
            .http
            .get(&url)
            .bearer_auth(self.databricks.token().await?)
            .send()
            .await
            .map_err(|e| Error::internal(format!("listing git credentials: {e}")))?;
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(Error::internal(format!(
                "listing git credentials: {status}: {}",
                message(&body)
            )));
        }
        let all = body
            .get("credentials")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let github = |c: &&Value| {
            c.get("git_provider")
                .and_then(Value::as_str)
                .is_some_and(|p| p.eq_ignore_ascii_case("github"))
        };
        let name = Self::name(owner);
        let ours = all
            .iter()
            .filter(github)
            .find(|c| c.get("name").and_then(Value::as_str) == Some(name.as_str()));
        let default = || {
            all.iter().filter(github).find(|c| {
                c.get("is_default_for_provider")
                    .and_then(Value::as_bool)
                    // Without multiple credentials there's no flag: the one is it.
                    .unwrap_or(true)
            })
        };
        Ok(ours
            .or_else(default)
            .and_then(|c| c.get("credential_id"))
            .and_then(Value::as_u64))
    }

    fn body(token: &InstallationToken) -> Value {
        json!({
            "git_provider": "gitHub",
            "git_username": "x-access-token",
            "personal_access_token": token.token,
            "name": Self::name(&token.owner),
            "is_default_for_provider": true,
        })
    }

    async fn update(&self, id: u64, token: &InstallationToken) -> Result<Write> {
        let url = format!("{}{PATH}/{id}", self.workspace);
        self.send(self.http.patch(&url), &Self::body(token), Some(id))
            .await
    }

    async fn create(&self, token: &InstallationToken) -> Result<Write> {
        let url = format!("{}{PATH}", self.workspace);
        self.send(self.http.post(&url), &Self::body(token), None)
            .await
    }

    /// Send `body`; the credential's id (`id`, or the created one's).
    async fn send(
        &self,
        request: reqwest::RequestBuilder,
        body: &Value,
        id: Option<u64>,
    ) -> Result<Write> {
        let response = request
            .bearer_auth(self.databricks.token().await?)
            .json(body)
            .send()
            .await
            .map_err(|e| Error::internal(format!("writing the git credential: {e}")))?;
        let status = response.status();
        let answer = response.json::<Value>().await.unwrap_or(Value::Null);
        if status.is_success() {
            return id
                .or_else(|| answer.get("credential_id").and_then(Value::as_u64))
                .map(Write::Done)
                .ok_or_else(|| Error::internal("a created git credential without an id"));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(Write::Missing);
        }
        if status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Ok(Write::Refused(format!("{status}: {}", message(&answer))));
        }
        Err(Error::internal(format!(
            "writing the git credential: {status}: {}",
            message(&answer)
        )))
    }

    /// Run the rounds until the task is aborted.
    pub fn spawn(mut self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut written: Option<DateTime<Utc>> = None;
            let mut wait = self.backoff.0;
            loop {
                let next = match self.refresh().await {
                    Ok(expires_at) => {
                        tracing::info!(%expires_at, id = ?self.credential_id, "wrote the git credential's token");
                        written = Some(expires_at);
                        wait = self.backoff.0;
                        let due = expires_at
                            - chrono::Duration::from_std(self.margin).unwrap_or_default();
                        (due - Utc::now()).to_std().unwrap_or_default()
                    }
                    Err(error) => {
                        let short = written.is_none_or(|at| {
                            at - Utc::now()
                                < chrono::Duration::from_std(GitHubApp::MARGIN).unwrap_or_default()
                        });
                        if short {
                            tracing::error!(%error, expires_at = ?written, retry = ?wait, "refreshing the git credential: sessions may lose GitHub");
                        } else {
                            tracing::warn!(%error, expires_at = ?written, retry = ?wait, "refreshing the git credential");
                        }
                        let this = wait;
                        wait = (wait * 2).min(self.backoff.1);
                        this
                    }
                };
                tokio::time::sleep(next).await;
            }
        })
    }
}

fn message(body: &Value) -> String {
    body.get("message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    use axum::extract::{Path, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::{get, patch, post};
    use factory_core::codehost::github::testing::test_key;
    use factory_core::token::StaticToken;

    /// A write the fake saw: method, credential id, bearer, body.
    type Written = (&'static str, Option<u64>, String, Value);

    /// A fake GitHub (the App's installations and tokens) and Databricks
    /// (git credentials), on one server.
    #[derive(Default)]
    struct Fake {
        /// Seconds each minted token lasts.
        lasts: AtomicI64,
        mints: AtomicUsize,
        listings: AtomicUsize,
        /// The credentials there are: (id, body).
        credentials: Mutex<Vec<(u64, Value)>>,
        /// Statuses the next writes answer, in order; then 200.
        write_replies: Mutex<Vec<StatusCode>>,
        /// Every write: (method, id, bearer, body).
        writes: Mutex<Vec<Written>>,
    }

    impl Fake {
        fn tokens(&self) -> Vec<String> {
            self.writes
                .lock()
                .unwrap()
                .iter()
                .map(|w| w.3["personal_access_token"].as_str().unwrap().to_owned())
                .collect()
        }

        fn methods(&self) -> Vec<(&'static str, Option<u64>)> {
            self.writes
                .lock()
                .unwrap()
                .iter()
                .map(|w| (w.0, w.1))
                .collect()
        }

        fn next_reply(&self) -> StatusCode {
            let mut replies = self.write_replies.lock().unwrap();
            if replies.is_empty() {
                StatusCode::OK
            } else {
                replies.remove(0)
            }
        }
    }

    fn bearer(h: &HeaderMap) -> String {
        h.get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or_default()
            .to_owned()
    }

    async fn serve(fake: Arc<Fake>) -> String {
        let app = axum::Router::new()
            .route(
                "/app/installations",
                get(|State(f): State<Arc<Fake>>| async move {
                    f.listings.fetch_add(1, Ordering::SeqCst);
                    axum::Json(json!([{"id": 7, "account": {"login": "Lakehouse"}}]))
                }),
            )
            .route(
                "/app/installations/{id}/access_tokens",
                post(|State(f): State<Arc<Fake>>, Path(id): Path<u64>| async move {
                    let n = f.mints.fetch_add(1, Ordering::SeqCst) + 1;
                    let at = Utc::now() + chrono::Duration::milliseconds(f.lasts.load(Ordering::SeqCst));
                    axum::Json(json!({"token": format!("ghs_{id}_{n}"), "expires_at": at.to_rfc3339()}))
                }),
            )
            .route(
                PATH,
                get(|State(f): State<Arc<Fake>>| async move {
                    let list: Vec<Value> = f
                        .credentials
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(id, c)| {
                            let mut c = c.clone();
                            c["credential_id"] = json!(id);
                            c.as_object_mut().unwrap().remove("personal_access_token");
                            c
                        })
                        .collect();
                    axum::Json(json!({ "credentials": list }))
                })
                .post(
                    |State(f): State<Arc<Fake>>, h: HeaderMap, axum::Json(body): axum::Json<Value>| async move {
                        f.writes.lock().unwrap().push(("POST", None, bearer(&h), body.clone()));
                        let status = f.next_reply();
                        if status != StatusCode::OK {
                            return (status, axum::Json(json!({"message": "no"})));
                        }
                        let mut credentials = f.credentials.lock().unwrap();
                        let id = 100 + credentials.len() as u64;
                        credentials.push((id, body));
                        (StatusCode::OK, axum::Json(json!({"credential_id": id})))
                    },
                ),
            )
            .route(
                &format!("{PATH}/{{id}}"),
                patch(
                    |State(f): State<Arc<Fake>>,
                     Path(id): Path<u64>,
                     h: HeaderMap,
                     axum::Json(body): axum::Json<Value>| async move {
                        f.writes.lock().unwrap().push(("PATCH", Some(id), bearer(&h), body.clone()));
                        let status = f.next_reply();
                        let mut credentials = f.credentials.lock().unwrap();
                        let Some(c) = credentials.iter_mut().find(|(i, _)| *i == id) else {
                            return (StatusCode::NOT_FOUND, axum::Json(json!({"message": "gone"})));
                        };
                        if status != StatusCode::OK {
                            return (status, axum::Json(json!({"message": "bad token"})));
                        }
                        c.1 = body;
                        (StatusCode::OK, axum::Json(json!({})))
                    },
                ),
            )
            .with_state(fake);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    /// A refresher over the fake, its tokens lasting `lasts_ms`.
    async fn setup(lasts_ms: i64) -> (Arc<Fake>, GitCredentialRefresher) {
        let fake = Arc::new(Fake::default());
        fake.lasts.store(lasts_ms, Ordering::SeqCst);
        let url = serve(fake.clone()).await;
        let app = GitHubApp::new("12345", test_key()).unwrap().with_api(&url);
        let refresher = GitCredentialRefresher::new(
            Arc::new(app),
            None,
            url,
            Arc::new(StaticToken::new("dbx-token")),
        )
        .with_margin(Duration::from_secs(60))
        .with_backoff(Duration::from_millis(20), Duration::from_millis(50));
        (fake, refresher)
    }

    const HOUR: i64 = 3_600_000;

    /// Wait (in real time) until `ok`, or fail after five seconds.
    async fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !ok() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {what}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn creates_the_credential_when_theres_none() {
        let (fake, mut refresher) = setup(HOUR).await;
        let expires_at = refresher.refresh().await.unwrap();
        assert!(expires_at - Utc::now() > chrono::Duration::minutes(59));
        let writes = fake.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1);
        let (method, _, bearer, body) = &writes[0];
        assert_eq!(*method, "POST");
        assert_eq!(bearer, "dbx-token", "written as the Databricks principal");
        assert_eq!(
            *body,
            json!({
                "git_provider": "gitHub",
                "git_username": "x-access-token",
                "personal_access_token": "ghs_7_1",
                "name": "factory-github-app:Lakehouse",
                "is_default_for_provider": true,
            })
        );
        // The next round updates the one it created.
        refresher.refresh().await.unwrap();
        assert_eq!(fake.methods(), [("POST", None), ("PATCH", Some(100))]);
        assert_eq!(fake.tokens(), ["ghs_7_1", "ghs_7_2"]);
        assert_eq!(
            fake.listings.load(Ordering::SeqCst),
            1,
            "the installation is remembered"
        );
        assert!(!format!("{refresher:?}").contains("dbx-token"));
    }

    #[tokio::test]
    async fn adopts_a_credential_by_name_or_the_default() {
        let (fake, mut refresher) = setup(HOUR).await;
        *fake.credentials.lock().unwrap() = vec![
            (
                1,
                json!({"git_provider": "gitLab", "is_default_for_provider": true}),
            ),
            (
                2,
                json!({"git_provider": "gitHub", "is_default_for_provider": true}),
            ),
            (
                3,
                json!({"git_provider": "gitHub", "name": "factory-github-app:Lakehouse"}),
            ),
        ];
        refresher.refresh().await.unwrap();
        assert_eq!(fake.methods(), [("PATCH", Some(3))], "ours, by name");

        let (fake, mut refresher) = setup(HOUR).await;
        *fake.credentials.lock().unwrap() = vec![(
            5,
            json!({"git_provider": "gitHub", "git_username": "someone"}),
        )];
        refresher.refresh().await.unwrap();
        assert_eq!(
            fake.methods(),
            [("PATCH", Some(5))],
            "the hand-made one, not a second"
        );
    }

    #[tokio::test]
    async fn writes_a_new_token_before_the_current_one_expires() {
        // Each token lasts the margin plus 300 ms: the next round is due then.
        let (fake, refresher) = setup(60_000 + 300).await;
        let task = refresher.spawn();
        eventually("two writes", || fake.writes.lock().unwrap().len() >= 2).await;
        task.abort();
        assert_eq!(fake.methods()[..2], [("POST", None), ("PATCH", Some(100))]);
        assert_eq!(fake.tokens()[..2], ["ghs_7_1", "ghs_7_2"]);
    }

    #[tokio::test]
    async fn a_failed_write_is_retried() {
        let (fake, refresher) = setup(HOUR).await;
        *fake.write_replies.lock().unwrap() = vec![
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::TOO_MANY_REQUESTS,
        ];
        let task = refresher.spawn();
        eventually("a created credential", || {
            !fake.credentials.lock().unwrap().is_empty()
        })
        .await;
        task.abort();
        assert_eq!(fake.methods(), [("POST", None); 3]);
        assert_eq!(fake.tokens(), ["ghs_7_1", "ghs_7_2", "ghs_7_3"]);
        // Nothing more is due for the hour.
        assert_eq!(fake.writes.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_refused_token_is_minted_again() {
        let (fake, mut refresher) = setup(HOUR).await;
        *fake.credentials.lock().unwrap() = vec![(9, json!({"git_provider": "gitHub"}))];
        *fake.write_replies.lock().unwrap() = vec![StatusCode::BAD_REQUEST];
        refresher.refresh().await.unwrap();
        assert_eq!(fake.methods(), [("PATCH", Some(9)), ("PATCH", Some(9))]);
        assert_eq!(fake.tokens(), ["ghs_7_1", "ghs_7_2"]);
        assert_eq!(
            fake.listings.load(Ordering::SeqCst),
            2,
            "the installation is looked up again"
        );

        // Refused twice is the round's failure.
        *fake.write_replies.lock().unwrap() = vec![StatusCode::BAD_REQUEST, StatusCode::FORBIDDEN];
        let err = refresher.refresh().await.unwrap_err().to_string();
        assert!(err.contains("refused a newly minted token"), "{err}");
    }

    #[tokio::test]
    async fn a_deleted_credential_is_created_again() {
        let (fake, mut refresher) = setup(HOUR).await;
        refresher.refresh().await.unwrap();
        fake.credentials.lock().unwrap().clear();
        refresher.refresh().await.unwrap();
        assert_eq!(
            fake.methods(),
            [("POST", None), ("PATCH", Some(100)), ("POST", None)]
        );
        assert_eq!(refresher.credential_id, Some(100));
    }
}
