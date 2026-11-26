//! Running as a Databricks App: the port the platform assigns, who the Apps
//! proxy says the caller is, and the caller's groups from the workspace.
//!
//! - [`listen_addr`]: `DATABRICKS_APP_PORT` on every interface, unless
//!   `--listen` says otherwise.
//! - [`require_caller`]: factory.v1 behind a [`CallerAuth`]. With `--apps-proxy`
//!   that's the proxy's forwarded identity
//!   ([`AppsProxy`](workgraph_mcp::auth::AppsProxy)) mapped to grants by the
//!   principals config, the same one `/mcp` uses. Writes are attributed to the
//!   caller, whatever `x-factory-actor` the client sent.
//! - [`ScimGroups`]: a caller's groups from the workspace's SCIM API, for
//!   `group:` entries; the proxy forwards none.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::{Request, State};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use factory_core::token::TokenSource;
use tokio::sync::Mutex;
use tokio::time::Instant;
use workgraph_mcp::auth::{CallerAuth, GroupSource, Principal, PrincipalKind};

/// The port Databricks Apps assigns the App.
pub const APP_PORT_ENV: &str = "DATABRICKS_APP_PORT";

/// Where to listen: `given` (`--listen`) when it's set; else, in a Databricks
/// App, `0.0.0.0:<DATABRICKS_APP_PORT>`, the proxy's way in; else loopback.
pub fn listen_addr(
    given: Option<SocketAddr>,
    app_port: Option<&str>,
) -> Result<SocketAddr, String> {
    if let Some(addr) = given {
        return Ok(addr);
    }
    match app_port.map(str::trim).filter(|p| !p.is_empty()) {
        Some(port) => port
            .parse::<u16>()
            .map(|port| SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
            .map_err(|_| format!("{APP_PORT_ENV} is a port, not {port:?}")),
        None => Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, 7410))),
    }
}

/// The header factory.v1 attributes writes to.
const ACTOR_HEADER: &str = "x-factory-actor";

/// factory.v1 behind `auth`: a request without a grant gets its rejection;
/// one with a grant is attributed to the grant's actor, replacing any
/// `x-factory-actor` the client sent.
pub async fn require_caller(
    State(auth): State<Arc<dyn CallerAuth>>,
    req: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = req.into_parts();
    match auth.grant(&parts).await {
        Ok(grant) => {
            let Ok(actor) = HeaderValue::from_str(&grant.actor) else {
                return Response::builder()
                    .status(500)
                    .body("the caller's name isn't a header value".into())
                    .unwrap_or_default();
            };
            parts.headers.insert(ACTOR_HEADER, actor);
            next.run(Request::from_parts(parts, body)).await
        }
        Err(rejection) => rejection.into_response(),
    }
}

/// A principal's groups from the workspace's SCIM API, cached for a few
/// minutes.
///
/// A person (`user:<email>`) whose token the proxy forwarded (the App has
/// user authorization scopes) is read as themselves, from `/Me` with that
/// token: a workspace shows a non-admin service principal, such as the App's
/// own, nobody else's groups, but it shows everyone theirs. `/Me` has to name
/// the same person, or the lookup fails. Otherwise, a person is looked up by
/// user name and a service principal (`service-account:<application id>`) by
/// application id, with `token` (the App's own service principal), which
/// only finds groups when that principal may read them.
pub struct ScimGroups {
    workspace: String,
    token: Arc<dyn TokenSource>,
    http: reqwest::Client,
    ttl: Duration,
    cache: Mutex<HashMap<Principal, (Instant, Vec<String>)>>,
}

impl ScimGroups {
    /// How long a principal's groups are reused.
    pub const TTL: Duration = Duration::from_secs(5 * 60);

    /// The SCIM API of the workspace at `workspace` (`https://…`).
    pub fn new(workspace: impl Into<String>, token: Arc<dyn TokenSource>) -> Self {
        Self {
            workspace: workspace.into().trim_end_matches('/').to_owned(),
            token,
            http: reqwest::Client::new(),
            ttl: Self::TTL,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Reuse a principal's groups for `ttl`.
    #[must_use]
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// A person's groups, read as them with their own `token`.
    async fn fetch_own(&self, principal: &Principal, token: &str) -> Result<Vec<String>, String> {
        let url = format!("{}/api/2.0/preview/scim/v2/Me", self.workspace);
        let response = self
            .http
            .get(&url)
            .bearer_auth(token)
            .query(&[("attributes", "userName,groups")])
            .send()
            .await
            .map_err(|e| format!("scim: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("scim: {url}: {status}"));
        }
        let body: serde_json::Value = response.json().await.map_err(|e| format!("scim: {e}"))?;
        let me = body
            .get("userName")
            .and_then(serde_json::Value::as_str)
            .map(str::to_lowercase);
        if me.as_deref() != Some(principal.name.as_str()) {
            return Err(format!(
                "scim: /Me is {}, not {principal}",
                me.as_deref().unwrap_or("nobody")
            ));
        }
        Ok(group_names(body.get("groups")))
    }

    async fn fetch(&self, principal: &Principal) -> Result<Vec<String>, String> {
        let (resource, attribute) = match principal.kind {
            PrincipalKind::User => ("Users", "userName"),
            PrincipalKind::ServiceAccount => ("ServicePrincipals", "applicationId"),
            _ => return Ok(Vec::new()),
        };
        let url = format!("{}/api/2.0/preview/scim/v2/{resource}", self.workspace);
        let filter = format!("{attribute} eq \"{}\"", principal.name.replace('"', ""));
        let token = self.token.token().await.map_err(|e| e.to_string())?;
        let response = self
            .http
            .get(&url)
            .bearer_auth(token)
            .query(&[("filter", filter.as_str()), ("attributes", "groups")])
            .send()
            .await
            .map_err(|e| format!("scim: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("scim: {url}: {status}"));
        }
        let body: serde_json::Value = response.json().await.map_err(|e| format!("scim: {e}"))?;
        Ok(group_names(body.pointer("/Resources/0/groups")))
    }
}

/// The `display` names of a SCIM `groups` attribute.
fn group_names(groups: Option<&serde_json::Value>) -> Vec<String> {
    groups
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| g.get("display").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect()
}

#[async_trait]
impl GroupSource for ScimGroups {
    async fn groups(
        &self,
        principal: &Principal,
        token: Option<&str>,
    ) -> Result<Vec<String>, String> {
        if let Some((at, groups)) = self.cache.lock().await.get(principal)
            && at.elapsed() < self.ttl
        {
            return Ok(groups.clone());
        }
        let groups = match (principal.kind, token) {
            (PrincipalKind::User, Some(token)) => self.fetch_own(principal, token).await?,
            _ => self.fetch(principal).await?,
        };
        self.cache
            .lock()
            .await
            .insert(principal.clone(), (Instant::now(), groups.clone()));
        Ok(groups)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_app_port_is_used_unless_listen_is_given() {
        let given: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        assert_eq!(listen_addr(Some(given), Some("8000")), Ok(given));
        assert_eq!(
            listen_addr(None, Some("8000")).unwrap().to_string(),
            "0.0.0.0:8000"
        );
        assert_eq!(
            listen_addr(None, None).unwrap().to_string(),
            "127.0.0.1:7410"
        );
        assert_eq!(
            listen_addr(None, Some(" ")).unwrap().to_string(),
            "127.0.0.1:7410"
        );
        assert!(listen_addr(None, Some("http")).is_err());
    }

    #[tokio::test]
    async fn scim_groups_are_read_by_kind_and_cached() {
        use axum::extract::{Path, Query};
        use axum::http::HeaderMap;
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let app = axum::Router::new().route(
            "/api/2.0/preview/scim/v2/{resource}",
            axum::routing::get(
                move |Path(resource): Path<String>,
                      Query(q): Query<HashMap<String, String>>,
                      headers: HeaderMap| {
                    let seen = seen.clone();
                    async move {
                        seen.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(headers["authorization"], "Bearer sp-token");
                        assert_eq!(q["attributes"], "groups");
                        let groups = match (resource.as_str(), q["filter"].as_str()) {
                            ("Users", r#"userName eq "ada@example.com""#) => {
                                serde_json::json!([{"display": "operators"}, {"display": "users"}])
                            }
                            ("ServicePrincipals", r#"applicationId eq "sp-1""#) => {
                                serde_json::json!([{"display": "agents"}])
                            }
                            _ => return axum::Json(serde_json::json!({"Resources": []})),
                        };
                        axum::Json(serde_json::json!({"Resources": [{"groups": groups}]}))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let token = Arc::new(factory_core::token::StaticToken::new("sp-token"));
        let scim = ScimGroups::new(format!("http://{addr}"), token);
        let ada = Principal::user("ada@example.com");
        assert_eq!(
            scim.groups(&ada, None).await.unwrap(),
            ["operators", "users"]
        );
        assert_eq!(
            scim.groups(&ada, None).await.unwrap(),
            ["operators", "users"]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "cached");
        let sp = Principal::service_account("sp-1");
        assert_eq!(scim.groups(&sp, None).await.unwrap(), ["agents"]);
        // A service account is looked up as the App, even with a token.
        let sp2 = Principal::service_account("sp-1");
        let fresh = ScimGroups::new(format!("http://{addr}"), scim.token.clone());
        assert_eq!(fresh.groups(&sp2, Some("any")).await.unwrap(), ["agents"]);
        let nobody = Principal::user("eve@example.com");
        assert!(scim.groups(&nobody, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_person_with_a_forwarded_token_is_read_as_themselves() {
        use axum::extract::Query;
        use axum::http::HeaderMap;
        let app = axum::Router::new().route(
            "/api/2.0/preview/scim/v2/Me",
            axum::routing::get(
                |Query(q): Query<HashMap<String, String>>, headers: HeaderMap| async move {
                    assert_eq!(q["attributes"], "userName,groups");
                    let me = match headers["authorization"].to_str().unwrap() {
                        "Bearer grace-token" => "Grace@example.com",
                        _ => "mallory@example.com",
                    };
                    axum::Json(serde_json::json!({
                        "userName": me,
                        "groups": [{"display": "operators"}, {"display": "users"}],
                    }))
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        // The App's token is never used for a person with their own.
        let token = Arc::new(factory_core::token::StaticToken::new("unused"));
        let scim = ScimGroups::new(format!("http://{addr}"), token);
        let grace = Principal::user("grace@example.com");
        assert_eq!(
            scim.groups(&grace, Some("grace-token")).await.unwrap(),
            ["operators", "users"]
        );
        // A token that isn't the caller's is refused.
        let ada = Principal::user("ada@example.com");
        let err = scim.groups(&ada, Some("mallory-token")).await.unwrap_err();
        assert!(err.contains("/Me is mallory@example.com"), "{err}");
    }
}
