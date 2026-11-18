//! The token endpoint: Databricks access tokens for the containers of an instance whose
//! environment takes its models from a Databricks CLI profile.
//!
//! The Databricks SDK has a credential strategy for a host that mints tokens on a
//! container's behalf, `metadata-service`: given `DATABRICKS_HOST` and
//! `DATABRICKS_METADATA_SERVICE_URL`, it `GET`s the URL (headers
//! `X-Databricks-Metadata-Version: 1`, `X-Databricks-Host: <host>`) for `{access_token,
//! token_type, expires_on}`, and again before the token expires. MLflow's gateway and
//! Omnigent's token helper both use it. The app serves that URL here, for each running
//! instance that uses a profile, answering with `databricks auth token -p <profile>`
//! ([`crate::databricks::token`]).
//!
//! - One listener for every instance, bound on first use where the containers reach the
//!   host ([`Reach`]); each instance has its own random path, minted when it starts and
//!   never stored. The URL reaches Compose's process environment as a secret
//!   ([`ProfileSource`]'s overlay), and so the containers by name; never SQLite, the
//!   instance directory or the rendered tree.
//! - A request whose `X-Databricks-Host` isn't the profile's host gets a 403.
//! - When the CLI can't mint a token (the refresh token expired or was revoked), it gets a
//!   503, and the instance shows "run `databricks auth login -p <profile>`" until a
//!   request succeeds again.
//! - When the instance stops, its path is dropped and the URL answers 404; with no
//!   instance left, the listener closes.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use olai_stack_runtime::{Overlay, SecretValue, Source, SourceCtx, SourceError};
use serde::{Deserialize, Serialize};

use crate::databricks::{self, METADATA_SERVICE_URL_VAR};
use crate::toolchain::System;

/// The runtime source instances bind to take their token endpoint's URL from.
pub const SOURCE: &str = "databricks-profile";

/// The SDK's metadata-service request headers.
pub const VERSION_HEADER: &str = "X-Databricks-Metadata-Version";
pub const HOST_HEADER: &str = "X-Databricks-Host";

/// Where the containers reach the endpoint: the address the listener binds, and the host
/// name the URL they get carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reach {
    pub bind: IpAddr,
    pub url_host: String,
}

impl Reach {
    /// The host's loopback, which Docker Desktop, OrbStack and Colima forward
    /// `host.docker.internal` to.
    pub fn loopback() -> Self {
        Reach {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            url_host: "host.docker.internal".into(),
        }
    }
}

/// Where containers reach the host, asking `docker` when it matters. On macOS (and Docker
/// Desktop for Linux), `host.docker.internal` is the host's loopback. A Linux engine has no
/// such name unless a service maps it (trestle's compose doesn't), so bind the default
/// bridge's gateway, the host's address on `docker0`, which every compose network routes
/// to, and name it by address.
pub fn reach(system: &dyn System, linux: bool) -> Reach {
    if !linux {
        return Reach::loopback();
    }
    let Some(docker) = system.which("docker") else {
        return Reach::loopback();
    };
    let answer = |args: &[&str]| {
        system
            .run(&docker, args)
            .ok()
            .filter(|o| o.success)
            .map(|o| o.stdout.trim().to_owned())
    };
    if answer(&["info", "--format", "{{.OperatingSystem}}"])
        .is_some_and(|os| os.contains("Docker Desktop"))
    {
        return Reach::loopback();
    }
    match answer(&[
        "network",
        "inspect",
        "bridge",
        "--format",
        "{{(index .IPAM.Config 0).Gateway}}",
    ])
    .and_then(|ip| ip.parse::<IpAddr>().ok())
    {
        Some(ip) => Reach {
            bind: ip,
            url_host: ip.to_string(),
        },
        None => Reach::loopback(),
    }
}

/// An instance whose profile can't mint tokens, as the Doctor lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProfileProblem {
    pub instance: String,
    pub profile: String,
    pub message: String,
}

#[derive(Clone, Debug)]
struct Entry {
    profile: String,
    /// Normalized ([`databricks::normalize_host`]).
    host: String,
    path: String,
    problem: Option<String>,
}

struct Listener {
    port: u16,
    task: tokio::task::AbortHandle,
}

struct Inner {
    system: Arc<dyn System + Send + Sync>,
    reach: Mutex<Option<Reach>>,
    linux: bool,
    /// By instance id.
    entries: Mutex<BTreeMap<String, Entry>>,
    listener: tokio::sync::Mutex<Option<Listener>>,
}

/// The endpoints of the running instances that use a profile.
#[derive(Clone)]
pub struct TokenEndpoints {
    inner: Arc<Inner>,
}

impl TokenEndpoints {
    /// Minting tokens with the CLI on `system`; on first use, it asks `system`'s `docker`
    /// where containers reach the host ([`reach`]).
    pub fn new(system: Arc<dyn System + Send + Sync>) -> Self {
        Self::build(system, None, cfg!(target_os = "linux"))
    }

    /// Reached at `reach`, without asking `docker` (tests).
    pub fn with_reach(system: Arc<dyn System + Send + Sync>, reach: Reach) -> Self {
        Self::build(system, Some(reach), false)
    }

    fn build(system: Arc<dyn System + Send + Sync>, reach: Option<Reach>, linux: bool) -> Self {
        TokenEndpoints {
            inner: Arc::new(Inner {
                system,
                reach: Mutex::new(reach),
                linux,
                entries: Mutex::new(BTreeMap::new()),
                listener: tokio::sync::Mutex::new(None),
            }),
        }
    }

    /// The URL `instance`'s containers fetch `profile`'s tokens from: the instance's
    /// endpoint, made now (with a fresh path) unless it has one for the same profile.
    pub async fn serve(&self, instance: &str, profile: &str, host: &str) -> Result<String, String> {
        let reach = self.reach().await;
        let port = self.listen(&reach).await?;
        let host = databricks::normalize_host(host);
        let path = {
            let mut entries = self.inner.entries.lock().unwrap();
            match entries.get(instance) {
                Some(e) if e.profile == profile && e.host == host => e.path.clone(),
                _ => {
                    let path = random_path()?;
                    entries.insert(
                        instance.to_owned(),
                        Entry {
                            profile: profile.to_owned(),
                            host,
                            path: path.clone(),
                            problem: None,
                        },
                    );
                    path
                }
            }
        };
        Ok(format!(
            "http://{}:{port}/{path}",
            url_host(&reach.url_host)
        ))
    }

    /// Drop `instance`'s endpoint: its URL answers 404 from now on. The listener closes
    /// with the last one.
    pub async fn remove(&self, instance: &str) {
        let empty = {
            let mut entries = self.inner.entries.lock().unwrap();
            entries.remove(instance);
            entries.is_empty()
        };
        if empty && let Some(listener) = self.inner.listener.lock().await.take() {
            listener.task.abort();
        }
    }

    /// The instances with an endpoint.
    pub fn instances(&self) -> Vec<String> {
        self.inner.entries.lock().unwrap().keys().cloned().collect()
    }

    /// Whether `instance` has an endpoint.
    pub fn serves(&self, instance: &str) -> bool {
        self.inner.entries.lock().unwrap().contains_key(instance)
    }

    /// Why `instance`'s profile can't mint tokens, if it can't.
    pub fn problem(&self, instance: &str) -> Option<String> {
        self.inner
            .entries
            .lock()
            .unwrap()
            .get(instance)
            .and_then(|e| e.problem.clone())
    }

    /// Every instance whose profile can't mint tokens.
    pub fn problems(&self) -> Vec<ProfileProblem> {
        self.inner
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(instance, e)| {
                e.problem.as_ref().map(|message| ProfileProblem {
                    instance: instance.clone(),
                    profile: e.profile.clone(),
                    message: message.clone(),
                })
            })
            .collect()
    }

    /// The URL the listener itself answers on for `instance` (tests, which can't resolve
    /// `host.docker.internal`).
    pub async fn local_url(&self, instance: &str) -> Option<String> {
        let reach = self.inner.reach.lock().unwrap().clone()?;
        let port = self.inner.listener.lock().await.as_ref()?.port;
        let path = self
            .inner
            .entries
            .lock()
            .unwrap()
            .get(instance)?
            .path
            .clone();
        Some(format!(
            "http://{}:{port}/{path}",
            url_host(&reach.bind.to_string())
        ))
    }

    async fn reach(&self) -> Reach {
        if let Some(reach) = self.inner.reach.lock().unwrap().clone() {
            return reach;
        }
        let system = Arc::clone(&self.inner.system);
        let linux = self.inner.linux;
        let reach = tokio::task::spawn_blocking(move || reach(system.as_ref(), linux))
            .await
            .unwrap_or_else(|_| Reach::loopback());
        *self.inner.reach.lock().unwrap() = Some(reach.clone());
        reach
    }

    /// The listener's port, binding it first if it isn't listening.
    async fn listen(&self, reach: &Reach) -> Result<u16, String> {
        let mut listener = self.inner.listener.lock().await;
        if let Some(l) = listener.as_ref()
            && !l.task.is_finished()
        {
            return Ok(l.port);
        }
        let socket = tokio::net::TcpListener::bind(SocketAddr::new(reach.bind, 0))
            .await
            .map_err(|e| format!("the token endpoint can't listen on {}: {e}", reach.bind))?;
        let port = socket.local_addr().map_err(|e| e.to_string())?.port();
        let app = axum::Router::new()
            .route("/{path}", axum::routing::get(handle))
            .with_state(Arc::clone(&self.inner));
        let task = tokio::spawn(async move {
            if let Err(e) = axum::serve(socket, app).await {
                tracing::warn!("the token endpoint stopped: {e}");
            }
        })
        .abort_handle();
        *listener = Some(Listener { port, task });
        Ok(port)
    }
}

fn url_host(host: &str) -> String {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => format!("[{v6}]"),
        _ => host.to_owned(),
    }
}

fn random_path() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no randomness for a token endpoint: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// How a request was answered, and what it means for the instance's problem.
#[derive(Debug, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub body: String,
    /// `Some(None)` clears the problem, `Some(Some(_))` sets it, `None` leaves it.
    pub problem: Option<Option<String>>,
}

/// Answer a metadata-service request for `profile` (at `profile_host`, normalized) that
/// sent `version` and `host` as its headers.
pub fn answer(
    system: &dyn System,
    profile: &str,
    profile_host: &str,
    version: Option<&str>,
    host: Option<&str>,
) -> Answer {
    let refuse = |status: u16, body: &str| Answer {
        status,
        body: body.to_owned(),
        problem: None,
    };
    if version != Some("1") {
        return refuse(400, "expected X-Databricks-Metadata-Version: 1");
    }
    let Some(host) = host else {
        return refuse(400, "expected X-Databricks-Host");
    };
    if databricks::normalize_host(host) != profile_host {
        return refuse(403, "this endpoint serves another workspace");
    }
    match databricks::token(system, profile) {
        Ok(token) => Answer {
            status: 200,
            body: serde_json::to_string(&token).expect("a token serializes"),
            problem: Some(None),
        },
        Err(e) => {
            tracing::warn!("no Databricks token for profile {profile}: {e}");
            let hint = databricks::login_hint(profile);
            Answer {
                status: 503,
                body: format!("the Databricks CLI can't refresh profile {profile}: {hint}"),
                problem: Some(Some(hint)),
            }
        }
    }
}

async fn handle(
    State(inner): State<Arc<Inner>>,
    UrlPath(path): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    let found = {
        let entries = inner.entries.lock().unwrap();
        entries
            .iter()
            .find(|(_, e)| constant_time_eq(e.path.as_bytes(), path.as_bytes()))
            .map(|(instance, e)| (instance.clone(), e.profile.clone(), e.host.clone()))
    };
    let Some((instance, profile, profile_host)) = found else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let (version, host) = (header(VERSION_HEADER), header(HOST_HEADER));
    let system = Arc::clone(&inner.system);
    let answered = tokio::task::spawn_blocking(move || {
        answer(
            system.as_ref(),
            &profile,
            &profile_host,
            version.as_deref(),
            host.as_deref(),
        )
    })
    .await;
    let Ok(answered) = answered else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if let Some(problem) = answered.problem
        && let Some(entry) = inner.entries.lock().unwrap().get_mut(&instance)
    {
        entry.problem = problem;
    }
    let status = StatusCode::from_u16(answered.status).unwrap_or(StatusCode::OK);
    let content_type = if status == StatusCode::OK {
        "application/json"
    } else {
        "text/plain; charset=utf-8"
    };
    (
        status,
        [(axum::http::header::CONTENT_TYPE, content_type)],
        answered.body,
    )
        .into_response()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// What an instance's binding to [`SOURCE`] says: the profile and its workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileBinding {
    pub profile: String,
    pub host: String,
}

/// The runtime source of an instance's `DATABRICKS_METADATA_SERVICE_URL`: its endpoint's
/// URL, as a credential, so it reaches Compose's process environment only.
pub struct ProfileSource {
    endpoints: TokenEndpoints,
}

impl ProfileSource {
    pub fn new(endpoints: TokenEndpoints) -> Self {
        ProfileSource { endpoints }
    }
}

#[async_trait]
impl Source for ProfileSource {
    fn name(&self) -> &str {
        SOURCE
    }

    async fn overlay(&self, ctx: &SourceCtx) -> Result<Overlay, SourceError> {
        let fail = |reason: String| SourceError {
            source_name: SOURCE.into(),
            reason,
        };
        let binding: ProfileBinding = serde_json::from_value(ctx.config.clone())
            .map_err(|e| fail(format!("no profile in the binding: {e}")))?;
        let url = self
            .endpoints
            .serve(&ctx.instance.0, &binding.profile, &binding.host)
            .await
            .map_err(fail)?;
        Ok(Overlay {
            credentials: [(METADATA_SERVICE_URL_VAR.to_owned(), SecretValue::new(url))].into(),
            ..Overlay::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::databricks::tests::{FakeCli, failed};

    const HOST: &str = "https://dbc-1.cloud.databricks.com";

    fn cli() -> FakeCli {
        let cli = FakeCli::new();
        cli.token("work", "dapi-secret", "2026-11-18T18:00:26+01:00");
        cli
    }

    #[test]
    fn a_good_request_gets_the_cli_s_token() {
        let a = answer(
            &cli(),
            "work",
            HOST,
            Some("1"),
            Some("https://dbc-1.cloud.databricks.com/"),
        );
        assert_eq!(a.status, 200);
        let body: serde_json::Value = serde_json::from_str(&a.body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "access_token": "dapi-secret",
                "token_type": "Bearer",
                "expires_on": 1_795_021_226u64,
            })
        );
        assert_eq!(a.problem, Some(None));
    }

    #[test]
    fn another_workspace_is_refused_without_asking_the_cli() {
        let cli = cli();
        let a = answer(
            &cli,
            "work",
            HOST,
            Some("1"),
            Some("https://dbc-2.cloud.databricks.com"),
        );
        assert_eq!(a.status, 403);
        assert!(!a.body.contains("dapi"));
        assert!(cli.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_request_without_the_headers_is_refused() {
        let cli = cli();
        assert_eq!(answer(&cli, "work", HOST, None, Some(HOST)).status, 400);
        assert_eq!(
            answer(&cli, "work", HOST, Some("2"), Some(HOST)).status,
            400
        );
        assert_eq!(answer(&cli, "work", HOST, Some("1"), None).status, 400);
        assert!(cli.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_cli_that_cant_refresh_answers_503_and_says_how_to_fix_it() {
        let cli = FakeCli::new();
        cli.answer(
            "auth token -p work -o json",
            Ok(failed("Error: a new access token could not be retrieved because the refresh token is invalid")),
        );
        let a = answer(&cli, "work", HOST, Some("1"), Some(HOST));
        assert_eq!(a.status, 503);
        assert_eq!(
            a.problem,
            Some(Some("run `databricks auth login -p work`".into()))
        );
    }

    #[test]
    fn docker_desktop_on_linux_is_reached_on_loopback_a_plain_engine_on_its_bridge() {
        use crate::databricks::tests::ok;
        use crate::toolchain::CommandOutput;
        use std::path::{Path, PathBuf};
        struct Docker(&'static str);
        impl System for Docker {
            fn which(&self, name: &str) -> Option<PathBuf> {
                (name == "docker").then(|| "/usr/bin/docker".into())
            }
            fn locate_neon(&self) -> Option<PathBuf> {
                None
            }
            fn run(&self, _: &Path, args: &[&str]) -> std::io::Result<CommandOutput> {
                match args[0] {
                    "info" => ok(self.0),
                    _ => ok("172.17.0.1\n"),
                }
            }
        }
        assert_eq!(reach(&Docker("Docker Desktop"), true), Reach::loopback());
        assert_eq!(
            reach(&Docker("Ubuntu 24.04.3 LTS"), true),
            Reach {
                bind: "172.17.0.1".parse().unwrap(),
                url_host: "172.17.0.1".into()
            }
        );
        assert_eq!(
            reach(&Docker("Ubuntu 24.04.3 LTS"), false),
            Reach::loopback()
        );
    }

    fn endpoints(cli: FakeCli) -> TokenEndpoints {
        TokenEndpoints::with_reach(
            Arc::new(cli),
            Reach {
                bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
                url_host: "host.docker.internal".into(),
            },
        )
    }

    async fn get(url: &str, host: &str) -> (u16, String) {
        let resp = reqwest::Client::new()
            .get(url)
            .header(VERSION_HEADER, "1")
            .header(HOST_HEADER, host)
            .send()
            .await
            .unwrap();
        (resp.status().as_u16(), resp.text().await.unwrap())
    }

    #[tokio::test]
    async fn each_instance_has_its_own_path_until_it_stops() {
        let e = endpoints(cli());
        let url = e.serve("i1", "work", HOST).await.unwrap();
        assert!(url.starts_with("http://host.docker.internal:"), "{url}");
        let path = url.rsplit('/').next().unwrap();
        assert_eq!(path.len(), 64, "32 random bytes, hex");
        // Asking again (a restart, a stop's re-render) keeps the URL.
        assert_eq!(e.serve("i1", "work", HOST).await.unwrap(), url);
        let other = e.serve("i2", "work", HOST).await.unwrap();
        assert_ne!(other, url);

        let local = e.local_url("i1").await.unwrap();
        let (status, body) = get(&local, HOST).await;
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("dapi-secret"));
        assert_eq!(get(&local, "https://evil.example").await.0, 403);

        let unknown = format!("{}/{}", local.rsplit_once('/').unwrap().0, "0".repeat(64));
        assert_eq!(get(&unknown, HOST).await.0, 404);

        e.remove("i1").await;
        assert_eq!(get(&local, HOST).await.0, 404);
        assert!(!e.serves("i1"));
        // The last one closes the listener.
        let local2 = e.local_url("i2").await.unwrap();
        e.remove("i2").await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            reqwest::Client::new().get(&local2).send().await.is_err(),
            "still listening"
        );
    }

    #[tokio::test]
    async fn a_failed_refresh_shows_on_the_instance_until_one_succeeds() {
        let cli = FakeCli::new();
        cli.answer(
            "auth token -p work -o json",
            Ok(failed("Error: refresh token is invalid")),
        );
        let cli = Arc::new(cli);
        let e = TokenEndpoints::with_reach(cli.clone(), Reach::loopback().with_bind_loopback());
        e.serve("i1", "work", HOST).await.unwrap();
        let local = e.local_url("i1").await.unwrap();
        assert_eq!(get(&local, HOST).await.0, 503);
        assert_eq!(
            e.problems(),
            [ProfileProblem {
                instance: "i1".into(),
                profile: "work".into(),
                message: "run `databricks auth login -p work`".into(),
            }]
        );
        cli.token("work", "dapi-fresh", "2026-11-18T18:00:26+01:00");
        assert_eq!(get(&local, HOST).await.0, 200);
        assert_eq!(e.problem("i1"), None);
    }

    impl Reach {
        fn with_bind_loopback(self) -> Self {
            Reach {
                bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
                ..self
            }
        }
    }
}
