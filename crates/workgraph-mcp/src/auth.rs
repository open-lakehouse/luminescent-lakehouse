//! Per-session bearer tokens.
//!
//! Each agent session gets its own token, minted by whoever starts the
//! session (the reconciler, or a composition by hand) and revoked when the
//! session's run ends. The token maps to a [`Grant`]: who the caller is, the
//! graph, run and node it works on, and the [`ToolPolicy`] it may use. The
//! server keeps only the tokens' hashes.
//!
//! The server holds no tracker or code-host credentials: every tool goes
//! through the [`Factory`](factory_core::Factory), so an agent's token can
//! only ever reach the work graph.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
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

fn unauthorized(why: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            format!("Bearer realm=\"workgraph-mcp\", error=\"invalid_token\", error_description=\"{why}\""),
        )],
        why.to_owned(),
    )
        .into_response()
}

/// The middleware in front of the MCP endpoint: every request needs a live
/// token, whose [`Grant`] it puts in the request's extensions for the
/// handler. Checked per request, so a revoked token stops working at once,
/// mid-session too.
pub async fn require_token(State(tokens): State<Tokens>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .map(str::trim);
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return unauthorized("a bearer token is required");
    };
    let Some(grant) = tokens.grant(token) else {
        return unauthorized("the token isn't valid");
    };
    req.extensions_mut().insert(grant);
    next.run(req).await
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
}
