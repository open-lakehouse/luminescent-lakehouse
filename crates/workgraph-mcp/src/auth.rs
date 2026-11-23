//! Who the caller is, and what it may do.
//!
//! Every request to the MCP endpoint goes through a [`CallerAuth`] first,
//! which turns it into a [`Grant`]: who the caller is, the graph, run and
//! node it works on, and the [`ToolPolicy`] it may use. Or it refuses the
//! request (a [`Rejection`]: 401, 403 or 503). There are two kinds:
//!
//! - [`Tokens`]: bearer tokens that each stand for a grant. factory-server
//!   mints one per agent session (the reconciler, or a composition by hand)
//!   and revokes it when the session's run ends, and it takes development
//!   tokens from its command line ([`Tokens::from_config`], the
//!   `[ROLE:]SECRET=POLICY` form of `FACTORY_MCP_TOKENS`). Only the tokens'
//!   hashes are kept.
//! - [`PrincipalAuth`]: an [`Authenticate`] that finds out who the caller is
//!   in the host's identity provider (a [`Principal`]), and a [`Principals`]
//!   map from configuration that decides what it may do. With the
//!   `olai-auth` feature, `olai::OlaiAuth` authenticates Neon Auth's users
//!   and service accounts' `olk_` API keys, as the hosted `workgraph`
//!   function does. [`AppsProxy`] takes the identity a Databricks App's
//!   proxy forwards, for factory-server running as an App.
//!
//! [`AnyOf`] puts a token set in front of another one: the hosted function
//! keeps the desktop's apps token (`FACTORY_MCP_APP_TOKENS`) that way, for now,
//! until the desktop signs in to Neon Auth.
//!
//! The server holds no tracker or code-host credentials: every tool goes
//! through the [`Factory`](factory_core::Factory), so a caller can only ever
//! reach the work graph.

pub mod apps;
#[cfg(feature = "olai-auth")]
pub mod olai;
mod principals;

pub use apps::{AppsProxy, GroupSource};

pub use principals::{
    APP_PRINCIPALS_ENV, Authenticate, CatalogRole, Identity, PRINCIPALS_ENV, Principal,
    PrincipalAuth, PrincipalKind, Principals, ROLES_ENV, RoleCatalog, Roles,
};

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use factory_core::tool::Caller;
use sha2::{Digest, Sha256};

use crate::policy::ToolPolicy;

/// What a token allows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grant {
    /// Who writes are attributed to.
    pub actor: String,
    /// A label for provenance and logs (`planner`, `operator`). The server
    /// never branches on it; the policy decides.
    pub role: String,
    /// The graph the session works on: tools default to it and refuse writes
    /// elsewhere. `None` for an unbound client (the operator's desktop).
    pub root: Option<String>,
    /// The session's run, attributed on writes and used by `report_change`.
    pub run: Option<String>,
    /// The node the session works on.
    pub node: Option<String>,
    pub policy: ToolPolicy,
    /// The right to MCP Apps: app-only tools (approval) and `ui://` views.
    /// Only an MCP Apps host acting for a person gets it (the desktop's side
    /// channel), never an agent's token, so no agent can approve its own
    /// work, whatever its client declares.
    pub apps: bool,
}

impl Grant {
    /// A grant for `actor` with `policy`, bound to nothing.
    pub fn new(actor: impl Into<String>, policy: ToolPolicy) -> Self {
        Self {
            actor: actor.into(),
            policy,
            ..Default::default()
        }
    }

    #[must_use]
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.role = role.into();
        self
    }

    #[must_use]
    pub fn root(mut self, root: impl Into<String>) -> Self {
        self.root = Some(root.into());
        self
    }

    #[must_use]
    pub fn run(mut self, run: impl Into<String>) -> Self {
        self.run = Some(run.into());
        self
    }

    #[must_use]
    pub fn node(mut self, node: impl Into<String>) -> Self {
        self.node = Some(node.into());
        self
    }

    /// Grant the right to MCP Apps (see [`Grant::apps`](Self::apps)).
    #[must_use]
    pub fn apps(mut self) -> Self {
        self.apps = true;
        self
    }

    /// The caller tools see.
    pub fn caller(&self) -> Caller {
        Caller {
            actor: self.actor.clone(),
            role: self.role.clone(),
            root: self.root.clone(),
            run: self.run.clone(),
            node: self.node.clone(),
        }
    }
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// The live tokens. Cheap to clone; clones share the set.
#[derive(Clone, Default)]
pub struct Tokens {
    grants: Arc<RwLock<HashMap<[u8; 32], Grant>>>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.grants.read().map(|g| g.len()).unwrap_or_default();
        f.debug_struct("Tokens").field("count", &n).finish()
    }
}

impl Tokens {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new random token for `grant`.
    pub fn mint(&self, grant: Grant) -> String {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("the OS has randomness");
        let token = format!("wg_{}", hex::encode(bytes));
        self.insert(&token, grant);
        token
    }

    /// Accept a token chosen elsewhere (a development token from the command
    /// line), replacing its grant if it's known.
    pub fn insert(&self, token: &str, grant: Grant) {
        self.write().insert(digest(token), grant);
    }

    /// Accept the token whose SHA-256 is `digest`, so the configuration
    /// needn't hold the token itself.
    pub fn insert_digest(&self, digest: [u8; 32], grant: Grant) {
        self.write().insert(digest, grant);
    }

    /// The token set of a configuration: agents' entries (`tokens`), then MCP
    /// Apps hosts' (`app_tokens`), each `[ROLE:]SECRET=POLICY`:
    ///
    /// - `SECRET` is the token, or `sha256:<64 hex>`, its digest.
    /// - With a `ROLE` (`planner:…`), the grant's actor and role are the
    ///   role: a gateway route's token, attributed to the route. Without
    ///   one, the actor is `operator` and the role `dev` (`app` for an app
    ///   token). A token can't contain `:` after its first character, then.
    /// - `POLICY` is a [`ToolPolicy`] (`@read,add_item`).
    ///
    /// App tokens get [`Grant::apps`]. Never put one on a gateway or proxy
    /// route: anyone behind the route could approve plans.
    pub fn from_config(tokens: &[String], app_tokens: &[String]) -> Result<Self, String> {
        let set = Self::new();
        for (spec, apps) in tokens
            .iter()
            .map(|s| (s, false))
            .chain(app_tokens.iter().map(|s| (s, true)))
        {
            let entry = Entry::parse(spec, apps)?;
            set.insert_digest(entry.digest, entry.grant);
        }
        Ok(set)
    }

    /// [`from_config`](Self::from_config) over `FACTORY_MCP_TOKENS` and
    /// `FACTORY_MCP_APP_TOKENS`, `;`-separated. Unset is empty: a server
    /// without tokens refuses every request.
    pub fn from_env() -> Result<Self, String> {
        Self::from_config(&env_list(TOKENS_ENV), &env_list(APP_TOKENS_ENV)).map_err(|e| {
            // The message names the entry's role and policy, never its secret.
            format!("{TOKENS_ENV}/{APP_TOKENS_ENV}: {e}")
        })
    }

    /// Only the apps tokens of `FACTORY_MCP_APP_TOKENS`: what a host that
    /// authenticates its callers as [`Principal`]s keeps for the desktop,
    /// until it signs in to the identity provider (interim).
    pub fn app_tokens_from_env() -> Result<Self, String> {
        Self::from_config(&[], &env_list(APP_TOKENS_ENV))
            .map_err(|e| format!("{APP_TOKENS_ENV}: {e}"))
    }

    /// How many tokens are live.
    pub fn len(&self) -> usize {
        self.grants.read().map(|g| g.len()).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Forget a token. Returns whether it was live.
    pub fn revoke(&self, token: &str) -> bool {
        self.write().remove(&digest(token)).is_some()
    }

    pub fn grant(&self, token: &str) -> Option<Grant> {
        self.grants
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&digest(token))
            .cloned()
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<[u8; 32], Grant>> {
        self.grants.write().unwrap_or_else(|e| e.into_inner())
    }
}

/// A `;`-separated list from the environment; unset is empty.
pub(crate) fn env_list(name: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_default()
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The agents' tokens, as [`Tokens::from_env`] reads them.
pub const TOKENS_ENV: &str = "FACTORY_MCP_TOKENS";
/// The MCP Apps hosts' tokens (the desktop's direct connection only).
pub const APP_TOKENS_ENV: &str = "FACTORY_MCP_APP_TOKENS";

const DIGEST_PREFIX: &str = "sha256:";

/// Whether `role` is a valid role name: ASCII letters, digits, `-` and `_`.
pub(crate) fn valid_role(role: &str) -> bool {
    !role.is_empty()
        && role
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// One `[ROLE:]SECRET=POLICY` entry.
struct Entry {
    digest: [u8; 32],
    grant: Grant,
}

impl Entry {
    fn parse(spec: &str, apps: bool) -> Result<Self, String> {
        let kind = if apps { "an app token" } else { "a token" };
        let (secret, policy) = spec
            .split_once('=')
            .ok_or_else(|| format!("{kind} wants [ROLE:]SECRET=POLICY, got no `=`"))?;
        let policy: ToolPolicy = policy.parse()?;
        let (role, secret) = match secret.split_once(':') {
            Some((role, rest)) if !secret.starts_with(DIGEST_PREFIX) => (Some(role), rest),
            _ => (None, secret),
        };
        if let Some(role) = role
            && !valid_role(role)
        {
            return Err(format!("{kind} has an invalid role {role:?}"));
        }
        let digest = match secret.strip_prefix(DIGEST_PREFIX) {
            Some(hex) => hex::decode(hex)
                .ok()
                .and_then(|b| <[u8; 32]>::try_from(b).ok())
                .ok_or_else(|| format!("{kind}'s sha256: wants 64 hex digits"))?,
            None if secret.is_empty() => return Err(format!("{kind} has an empty secret")),
            None => digest(secret),
        };
        let grant = match role {
            Some(role) => Grant::new(role, policy).role(role),
            None if apps => Grant::new("operator", policy).role("app"),
            None => Grant::new("operator", policy).role("dev"),
        };
        let grant = if apps { grant.apps() } else { grant };
        Ok(Self { digest, grant })
    }
}

/// Why a request was refused, and the response that says so.
pub struct Rejection {
    status: StatusCode,
    response: Response,
}

impl std::fmt::Debug for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rejection")
            .field("status", &self.status)
            .finish()
    }
}

impl Rejection {
    /// A 401 with an RFC 6750 `Bearer` challenge.
    pub fn unauthorized(why: &str) -> Self {
        let response = (
            StatusCode::UNAUTHORIZED,
            [(
                header::WWW_AUTHENTICATE,
                format!(
                    "Bearer realm=\"workgraph-mcp\", error=\"invalid_token\", error_description=\"{why}\""
                ),
            )],
            why.to_owned(),
        )
            .into_response();
        Self::from_response(response)
    }

    /// A 403: the caller is known, but may not use this server.
    pub fn forbidden(why: &str) -> Self {
        Self::from_response((StatusCode::FORBIDDEN, why.to_owned()).into_response())
    }

    /// A 503: the identity provider couldn't be asked.
    pub fn unavailable(why: &str) -> Self {
        Self::from_response((StatusCode::SERVICE_UNAVAILABLE, why.to_owned()).into_response())
    }

    /// A rejection that answers with `response` (an authenticator's own
    /// challenge, say).
    pub fn from_response(response: Response) -> Self {
        Self {
            status: response.status(),
            response,
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        self.response
    }
}

/// Turns a request into the caller's [`Grant`], or refuses it. The seam in
/// front of the MCP transport ([`crate::http::router`]): checked on every
/// request, so a revoked credential stops working at once, mid-session too.
#[async_trait]
pub trait CallerAuth: Send + Sync + 'static {
    async fn grant(&self, req: &Parts) -> Result<Grant, Rejection>;
}

#[async_trait]
impl<T: CallerAuth + ?Sized> CallerAuth for Arc<T> {
    async fn grant(&self, req: &Parts) -> Result<Grant, Rejection> {
        (**self).grant(req).await
    }
}

/// The request's bearer token, if it has a non-empty one.
pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

#[async_trait]
impl CallerAuth for Tokens {
    async fn grant(&self, req: &Parts) -> Result<Grant, Rejection> {
        let token = bearer(&req.headers)
            .ok_or_else(|| Rejection::unauthorized("a bearer token is required"))?;
        Tokens::grant(self, token).ok_or_else(|| Rejection::unauthorized("the token isn't valid"))
    }
}

/// A token set in front of another [`CallerAuth`]: a bearer that is one of
/// `tokens` gets its grant; any other request is `rest`'s to decide. The
/// hosted function keeps the desktop's apps token this way, next to the
/// principals (interim, until the desktop signs in to Neon Auth).
pub struct AnyOf<A> {
    pub tokens: Tokens,
    pub rest: A,
}

#[async_trait]
impl<A: CallerAuth> CallerAuth for AnyOf<A> {
    async fn grant(&self, req: &Parts) -> Result<Grant, Rejection> {
        if let Some(grant) = bearer(&req.headers).and_then(|t| self.tokens.grant(t)) {
            return Ok(grant);
        }
        self.rest.grant(req).await
    }
}

/// The middleware in front of the MCP endpoint: every request needs a
/// [`Grant`] from `auth`, which it puts in the request's extensions for the
/// handler.
pub async fn require_grant(
    State(auth): State<Arc<dyn CallerAuth>>,
    req: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = req.into_parts();
    match auth.grant(&parts).await {
        Ok(grant) => {
            parts.extensions.insert(grant);
            next.run(Request::from_parts(parts, body)).await
        }
        Err(rejection) => rejection.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_mint_and_revoke() {
        let tokens = Tokens::new();
        let grant = Grant::new("executor", ToolPolicy::read_only()).run("r1");
        let t = tokens.mint(grant.clone());
        assert!(t.starts_with("wg_") && t.len() == 3 + 64);
        assert_eq!(tokens.grant(&t), Some(grant));
        assert_ne!(t, tokens.mint(Grant::default()));
        assert!(tokens.revoke(&t));
        assert!(!tokens.revoke(&t));
        assert_eq!(tokens.grant(&t), None);
    }

    fn cfg(tokens: &[&str], apps: &[&str]) -> Result<Tokens, String> {
        let own = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        Tokens::from_config(&own(tokens), &own(apps))
    }

    #[test]
    fn config_tokens_parse_and_only_app_tokens_get_apps() {
        let set = cfg(&["dev=*"], &["desk=*,!submit"]).unwrap();
        assert_eq!(set.len(), 2);
        let agent = set.grant("dev").unwrap();
        assert_eq!(
            (agent.actor.as_str(), agent.role.as_str(), agent.apps),
            ("operator", "dev", false)
        );
        let host = set.grant("desk").unwrap();
        assert_eq!((host.role.as_str(), host.apps), ("app", true));
        assert_eq!(host.policy.to_string(), "*,!submit");

        assert!(cfg(&[], &["=*"]).unwrap_err().contains("app token"));
        assert!(cfg(&["dev"], &[]).is_err());
        assert!(cfg(&["dev=@nope"], &[]).is_err());
    }

    #[test]
    fn config_roles_name_the_grant() {
        let set = cfg(
            &[
                "planner:wg_p=@read,add_item",
                "reviewer:wg_r=@read,annotate",
            ],
            &[],
        )
        .unwrap();
        let planner = set.grant("wg_p").unwrap();
        assert_eq!(
            (planner.actor.as_str(), planner.role.as_str()),
            ("planner", "planner")
        );
        assert_eq!(planner.policy.to_string(), "@read,add_item");
        assert!(!planner.apps);
        // The role isn't part of the token.
        assert!(set.grant("planner:wg_p").is_none());
        assert_eq!(set.grant("wg_r").unwrap().role, "reviewer");
        // A token with a `:` reads as ROLE:SECRET.
        assert!(cfg(&["a b:x=*"], &[]).is_err());
        assert!(cfg(&[":x=*"], &[]).is_err());
        assert!(cfg(&["planner:=*"], &[]).is_err());
    }

    #[test]
    fn config_digests_stand_for_tokens() {
        let hex = hex::encode(digest("wg_secret"));
        let set = cfg(
            &[&format!("executor:sha256:{hex}=@read")],
            &[&format!("sha256:{hex}x=*")],
        );
        assert!(set.unwrap_err().contains("64 hex"));
        let set = cfg(&[&format!("executor:sha256:{hex}=@read")], &[]).unwrap();
        assert_eq!(set.grant("wg_secret").unwrap().role, "executor");
        let set = cfg(&[], &[&format!("sha256:{hex}=*")]).unwrap();
        let host = set.grant("wg_secret").unwrap();
        assert!(host.apps);
        assert_eq!(host.actor, "operator");
        assert!(cfg(&["sha256:abcd=*"], &[]).is_err());
    }
}
