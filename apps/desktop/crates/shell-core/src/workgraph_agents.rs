//! An environment's agents on the work graph: one egress route per role to the bound
//! branch's `workgraph` function, each carrying the API key of a service account made for
//! this environment and that role.
//!
//! What a role is, the server says. The function publishes its roles and the tools each
//! reaches (`GET /roles`, workgraph-mcp's role catalog). The roles chosen here become
//! routes whose tool allowlist is the catalog's, so a route can't ask for more than the
//! server grants. The pieces:
//!
//! - **Routes** ([`routes_for`]): `workgraph-<role>` to `function = "workgraph"`, path
//!   `/mcp`, consumed by Omnigent, with a bearer credential from `WORKGRAPH_<ROLE>_API_KEY`.
//!   trestle renders them into the egress gateway and Omnigent's MCP registry. A session
//!   selects its route by name (`mcp_registry_services`) and holds no token.
//! - **Identities** ([`provision`]): per role, a service account `factory-<role>-<env>` on
//!   the branch, in the group `factory-<role>s` that the function's
//!   `FACTORY_MCP_PRINCIPALS` maps to the role. Its key goes straight into the keychain and
//!   the environment's local vars, as the minted secrets do ([`crate::secrets`]). It is
//!   never returned or shown.
//! - **Rotation** ([`rotate`]): a new key into the keychain, the running instance's
//!   `secret-server` recreated ([`Runner::recreate_secret_server`](crate::instances::Runner::recreate_secret_server)),
//!   then the old key revoked. The egress gateway keeps running.
//!
//! **No apps token on a route.** Everything behind a route gets what its credential
//! grants, so a route carrying the desktop's apps token (or any `FACTORY_MCP_APP_*` value)
//! would let an agent approve its own plan. [`check_route_credential`] refuses such a
//! secret by name, and [`check_key_value`] refuses any value that isn't a service account's
//! `olk_` key. Start checks every route (a hand-edited `env.toml` included).

use async_trait::async_trait;
use olai_stack_state::EnvironmentId;
use olai_stack_topology::{EgressRoute, InjectedCredential, McpRoute, ModuleId, Selection};
use serde::{Deserialize, Serialize};

use crate::neon::SecretSink;
use crate::state::{RevisionInfo, SharedState, StateError, revision_info, validate};
use crate::vars::{VarInput, VarTarget};

/// The function the routes reach, by its slug on the branch.
pub const FUNCTION: &str = "workgraph";
/// Where the function serves MCP.
pub const MCP_PATH: &str = "/mcp";
/// Where the function publishes its role catalog.
pub const ROLES_PATH: &str = "/roles";
/// The module that calls the routes: Omnigent's server keeps the MCP registry.
pub const CONSUMER: &str = "omnigent";
/// The prefix of the routes this module owns.
pub const ROUTE_PREFIX: &str = "workgraph-";

/// Secrets that must never be a route's credential: the desktop's apps token and the
/// function's apps configuration, which carry the right to approve a plan.
const FORBIDDEN_SECRETS: &[&str] = &["FACTORY_MCP_APP_", "LUMINESCENT_SERVICE_TOKENS"];

type Result<T> = std::result::Result<T, StateError>;

fn invalid(message: impl Into<String>) -> StateError {
    StateError::Invalid {
        message: message.into(),
    }
}

/// One role of the function's catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CatalogRole {
    pub name: String,
    /// The policy as the function is configured with it (`@read,annotate`).
    #[serde(default)]
    pub policy: String,
    /// The tools the role reaches, by name.
    pub tools: Vec<String>,
}

/// The function's `GET /roles`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RoleCatalog {
    pub roles: Vec<CatalogRole>,
}

/// A client for [`fetch_roles`], with a timeout: a branch's function may be waking up.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default()
}

/// Fetch the role catalog of the function at `function_url` (its invocation URL).
pub async fn fetch_roles(
    http: &reqwest::Client,
    function_url: &str,
) -> std::result::Result<RoleCatalog, String> {
    let url = format!("{}{ROLES_PATH}", function_url.trim_end_matches('/'));
    let response = http
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("asking {url} for its roles: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "{url} answered {}: deploy the workgraph function with FACTORY_MCP_ROLES",
            response.status()
        ));
    }
    response
        .json()
        .await
        .map_err(|e| format!("{url}'s roles: {e}"))
}

/// The upper-cased form of `role` in a variable name (`planner` → `PLANNER`).
fn upper(role: &str) -> String {
    role.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// The route of `role`: `workgraph-<role>`.
pub fn route_name(role: &str) -> String {
    format!("{ROUTE_PREFIX}{role}")
}

/// The variable holding `role`'s key: `WORKGRAPH_<ROLE>_API_KEY`.
pub fn secret_var(role: &str) -> String {
    format!("WORKGRAPH_{}_API_KEY", upper(role))
}

/// The group the function maps to `role`: `factory-<role>s`.
pub fn group(role: &str) -> String {
    format!("factory-{role}s")
}

/// The service account of `role` for environment `env` (named `env_name`): lowercase,
/// within `authz`'s name rule (`^[a-z0-9][a-z0-9._-]{0,62}$`), unique per environment by
/// its id's first characters.
pub fn account(role: &str, env_name: &str, env: &str) -> String {
    let slug = |s: &str| -> String {
        let mut out = String::new();
        for c in s.chars() {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_alphanumeric() {
                out.push(c);
            } else if !out.ends_with('-') {
                out.push('-');
            }
        }
        out.trim_matches('-').to_owned()
    };
    let id: String = slug(env).chars().filter(|c| *c != '-').take(8).collect();
    let mut name = format!("factory-{}-{}", slug(role), slug(env_name));
    name.truncate(63 - id.len() - 1);
    format!("{}-{id}", name.trim_end_matches('-'))
}

/// A route this module owns: to the `workgraph` function, named `workgraph-<role>`.
pub fn is_workgraph_route(route: &EgressRoute) -> bool {
    route.function.as_deref() == Some(FUNCTION) && route.name.starts_with(ROUTE_PREFIX)
}

/// The role of an owned route.
pub fn role_of(route: &EgressRoute) -> Option<&str> {
    is_workgraph_route(route)
        .then(|| route.name.strip_prefix(ROUTE_PREFIX))
        .flatten()
}

/// One route per role, its tool allowlist the catalog's.
pub fn routes_for(roles: &[CatalogRole]) -> Vec<EgressRoute> {
    roles
        .iter()
        .map(|role| EgressRoute {
            name: route_name(&role.name),
            function: Some(FUNCTION.into()),
            upstream: None,
            path: MCP_PATH.into(),
            consumers: vec![ModuleId(CONSUMER.into())],
            credential: InjectedCredential::Bearer {
                secret: secret_var(&role.name),
            },
            mcp: Some(McpRoute {
                title: format!("Work graph ({})", role.name),
                tools: role.tools.clone(),
            }),
        })
        .collect()
}

/// Replace the routes this module owns in `selection` with `routes`, keeping every other.
pub fn replace_routes(selection: &mut Selection, routes: Vec<EgressRoute>) {
    selection.egress.routes.retain(|r| !is_workgraph_route(r));
    selection.egress.routes.extend(routes);
}

/// Refuse a route whose credential is the apps token or the function's apps
/// configuration: everything behind the route would get the right to approve plans.
pub fn check_route_credential(route: &EgressRoute) -> std::result::Result<(), String> {
    let secret = route.credential.secret_var();
    if FORBIDDEN_SECRETS
        .iter()
        .any(|forbidden| secret.starts_with(forbidden))
    {
        return Err(format!(
            "route {} carries {secret}, the desktop's apps credential: it would let every \
             agent behind the route approve its own plan. Give the route a service account's \
             API key",
            route.name
        ));
    }
    Ok(())
}

/// [`check_route_credential`] over every route of `selection`.
pub fn check_routes(selection: &Selection) -> std::result::Result<(), String> {
    selection
        .egress
        .routes
        .iter()
        .try_for_each(check_route_credential)
}

/// Refuse a key that isn't a service account's API key (`olk_<16 hex>_<64 hex>`): the
/// apps token (`wg_…`), a Neon Auth token, anything pasted by mistake.
pub fn check_key_value(value: &str) -> std::result::Result<(), String> {
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit());
    let ok = value
        .strip_prefix("olk_")
        .and_then(|rest| rest.split_once('_'))
        .is_some_and(|(id, secret)| hex(id, 16) && hex(secret, 64));
    if ok {
        Ok(())
    } else {
        Err(
            "that isn't a service account's API key (olk_…): a route takes only an \
             agent's key, never the apps token"
                .into(),
        )
    }
}

/// An egress route as the environment shows it. Never its credential's value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteView {
    pub name: String,
    /// The bound branch's function it reaches (one of `function` and `upstream`).
    pub function: Option<String>,
    /// The upstream origin it reaches.
    pub upstream: Option<String>,
    pub path: String,
    pub consumers: Vec<String>,
    /// The variable its credential is read from.
    pub secret: String,
    /// Whether that variable is set (on this machine, the environment or the workspace).
    pub key_set: bool,
    /// The tools an MCP route allows.
    pub tools: Vec<String>,
    /// The work-graph role it is for, when this module owns it.
    pub role: Option<String>,
}

/// The routes of `selection`, with whether each one's secret is set (`is_set`).
pub fn route_views(selection: &Selection, is_set: impl Fn(&str) -> bool) -> Vec<RouteView> {
    selection
        .egress
        .routes
        .iter()
        .map(|r| {
            let secret = r.credential.secret_var().to_owned();
            RouteView {
                name: r.name.clone(),
                function: r.function.clone(),
                upstream: r.upstream.clone(),
                path: r.path.clone(),
                consumers: r.consumers.iter().map(|m| m.0.clone()).collect(),
                key_set: is_set(&secret),
                secret,
                tools: r.mcp.as_ref().map(|m| m.tools.clone()).unwrap_or_default(),
                role: role_of(r).map(str::to_owned),
            }
        })
        .collect()
}

/// How the owned routes differ from the catalog: a role gone from the function, or a
/// route whose tools aren't the role's any more. Empty when they agree.
pub fn drift(selection: &Selection, catalog: &RoleCatalog) -> Vec<String> {
    let mut out = Vec::new();
    for route in selection
        .egress
        .routes
        .iter()
        .filter(|r| is_workgraph_route(r))
    {
        let role = role_of(route).unwrap_or_default();
        let tools = route.mcp.as_ref().map(|m| &m.tools);
        match catalog.roles.iter().find(|r| r.name == role) {
            None => out.push(format!("{}: the function has no role {role}", route.name)),
            Some(r) if tools != Some(&r.tools) => out.push(format!(
                "{}: the function's {role} reaches {}",
                route.name,
                r.tools.join(", ")
            )),
            Some(_) => {}
        }
    }
    out
}

/// What [`SharedState::set_workgraph_routes`] did.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RoutesApplied {
    /// The revision it committed, when the routes were all the draft changed from the head.
    pub committed: Option<RevisionInfo>,
    /// The draft has the routes, and other changes too: commit it before Start.
    pub needs_commit: bool,
}

impl SharedState {
    /// Environment `id`'s draft selection.
    pub fn draft_selection(&self, id: &str) -> Result<Selection> {
        Ok(self
            .store()
            .environment(&EnvironmentId(id.to_owned()))?
            .draft
            .manifest
            .selection)
    }

    /// Environment `id`'s name.
    pub fn environment_name(&self, id: &str) -> Result<String> {
        Ok(self
            .store()
            .environment(&EnvironmentId(id.to_owned()))?
            .name)
    }

    /// Whether `var` is set for environment `id`: in this machine's local vars, the
    /// environment's or the workspace's.
    pub fn var_is_set(&self, id: &str, var: &str) -> Result<bool> {
        let store = self.store();
        let env = store.environment(&EnvironmentId(id.to_owned()))?;
        let ws = store.workspace_by_id(&env.workspace)?;
        let local = store.local_vars(&env.id)?;
        Ok([&local, &env.vars, &ws.vars]
            .iter()
            .any(|vars| vars.get(var).is_some()))
    }

    /// Put `routes` (none: take them out) as the routes this module owns into environment
    /// `id`'s draft, and commit the draft when they are all it changes from the head.
    /// Refused unless the draft runs Omnigent, which consumes them.
    pub fn set_workgraph_routes(
        &self,
        id: &str,
        routes: Vec<EgressRoute>,
    ) -> Result<RoutesApplied> {
        let store = self.store();
        let env_id = EnvironmentId(id.to_owned());
        let env = store.environment(&env_id)?;
        if !routes.is_empty()
            && !env
                .draft
                .manifest
                .selection
                .modules
                .iter()
                .any(|m| m.0 == CONSUMER)
        {
            return Err(invalid(
                "the agents reach the work graph through Omnigent: add Omnigent to the \
                 environment first",
            ));
        }
        for route in &routes {
            check_route_credential(route).map_err(invalid)?;
        }
        let mut draft = env.draft.clone();
        replace_routes(&mut draft.manifest.selection, routes.clone());
        if draft != env.draft {
            store.modify_draft(&env_id, |d| {
                replace_routes(&mut d.manifest.selection, routes.clone());
                Ok::<_, StateError>(())
            })?;
        }
        let Some(head) = env.head else {
            return Ok(RoutesApplied {
                committed: None,
                needs_commit: false,
            });
        };
        let head = store.revision(&env_id, head)?;
        let mut wanted = head.manifest().clone();
        replace_routes(&mut wanted.selection, routes);
        if wanted == *head.manifest() {
            return Ok(RoutesApplied {
                committed: None,
                needs_commit: false,
            });
        }
        let clean = wanted == draft.manifest
            && head.modules() == &draft.modules
            && head.vars() == &draft.vars;
        if !clean {
            return Ok(RoutesApplied {
                committed: None,
                needs_commit: true,
            });
        }
        validate(&draft)?;
        let revision = store.commit_draft_with(&env_id, Some("Work graph agents"))?;
        Ok(RoutesApplied {
            committed: Some(revision_info(&revision)),
            needs_commit: false,
        })
    }

    /// Keep `value` as environment `id`'s `var`: in `keychain`, under the CLI's key, with a
    /// reference in this machine's local vars. Refused unless it is a service account's
    /// key. The value is never returned.
    pub fn store_route_key(
        &self,
        id: &str,
        var: &str,
        value: &str,
        keychain: &dyn SecretSink,
    ) -> Result<()> {
        check_key_value(value).map_err(invalid)?;
        if FORBIDDEN_SECRETS.iter().any(|f| var.starts_with(f)) {
            return Err(invalid(format!("{var} is never a route's credential")));
        }
        self.set_var(
            VarTarget::Local {
                environment: id.to_owned(),
            },
            var,
            VarInput::Secret(value.to_owned()),
            Some(keychain),
        )?;
        Ok(())
    }

    /// Forget environment `id`'s `var` (its keychain entry too).
    pub fn forget_route_key(&self, id: &str, var: &str, keychain: &dyn SecretSink) -> Result<()> {
        let local = self.store().local_vars(&EnvironmentId(id.to_owned()))?;
        if local.get(var).is_none() {
            return Ok(());
        }
        self.unset_var(
            &VarTarget::Local {
                environment: id.to_owned(),
            },
            var,
            Some(keychain),
        )
    }
}

/// A key just issued: its id (to revoke it by) and the key, which only the keychain keeps.
pub struct IssuedKey {
    pub id: String,
    pub key: String,
}

impl std::fmt::Debug for IssuedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuedKey")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// The `authz` operations provisioning needs, on the bound branch.
#[async_trait]
pub trait AuthzAdmin: Send + Sync {
    /// Create the service account unless it exists (refused if it was disabled).
    async fn ensure_account(
        &self,
        name: &str,
        description: &str,
    ) -> std::result::Result<(), String>;
    /// Put the account in the group (which the function's deploy created).
    async fn add_to_group(&self, group: &str, account: &str) -> std::result::Result<(), String>;
    /// Take the account out of the group.
    async fn remove_from_group(
        &self,
        group: &str,
        account: &str,
    ) -> std::result::Result<(), String>;
    async fn issue_key(
        &self,
        account: &str,
        description: &str,
    ) -> std::result::Result<IssuedKey, String>;
    async fn revoke_key(&self, id: &str) -> std::result::Result<(), String>;
}

/// [`AuthzAdmin`] over a branch's `authz` schema (luminescent-authz-admin), through `pool`
/// on its main database.
pub struct PgAuthzAdmin {
    pool: sqlx::PgPool,
}

impl PgAuthzAdmin {
    /// One connection to `url`, the branch's main database (direct, not pooled).
    pub async fn connect(url: &str) -> std::result::Result<Self, String> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(15))
            .connect(url)
            .await
            .map_err(|e| format!("connecting to the branch's database: {e}"))?;
        Ok(Self { pool })
    }

    pub async fn close(self) {
        self.pool.close().await;
    }
}

fn admin_err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[async_trait]
impl AuthzAdmin for PgAuthzAdmin {
    async fn ensure_account(
        &self,
        name: &str,
        description: &str,
    ) -> std::result::Result<(), String> {
        luminescent_authz_admin::admin::ensure_service_account(&self.pool, name, Some(description))
            .await
            .map(|_| ())
            .map_err(admin_err)
    }

    async fn add_to_group(&self, group: &str, account: &str) -> std::result::Result<(), String> {
        let member = luminescent_authz_admin::admin::PrincipalRef::ServiceAccount(account.into());
        luminescent_authz_admin::admin::add_member(&self.pool, group, &member)
            .await
            .map_err(|e| {
                format!(
                    "{}: create the group on the branch (`just admin <branch> group create {group}`) \
                     and map it in the function's FACTORY_MCP_PRINCIPALS",
                    admin_err(e)
                )
            })
    }

    async fn remove_from_group(
        &self,
        group: &str,
        account: &str,
    ) -> std::result::Result<(), String> {
        let member = luminescent_authz_admin::admin::PrincipalRef::ServiceAccount(account.into());
        luminescent_authz_admin::admin::remove_member(&self.pool, group, &member)
            .await
            .map_err(admin_err)
    }

    async fn issue_key(
        &self,
        account: &str,
        description: &str,
    ) -> std::result::Result<IssuedKey, String> {
        let key = luminescent_authz_admin::admin::issue_api_key(
            &self.pool,
            account,
            &[],
            None,
            Some(description),
        )
        .await
        .map_err(admin_err)?;
        Ok(IssuedKey {
            id: key.id,
            key: key.key,
        })
    }

    async fn revoke_key(&self, id: &str) -> std::result::Result<(), String> {
        luminescent_authz_admin::admin::revoke_api_key(&self.pool, id)
            .await
            .map_err(admin_err)
    }
}

/// What provisioning made for one role, which the app remembers (never the key).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", default)]
pub struct AgentIdentity {
    pub role: String,
    /// The service account, on the branch.
    pub account: String,
    /// Its group.
    pub group: String,
    /// The live key's id, when the app issued it (a pasted key has none here).
    pub key_id: Option<String>,
    /// The branch it is on, by id or name.
    pub branch: String,
}

/// One environment's agents on its branch: the store and keychain on this side, the
/// branch's `authz` on the other.
pub struct Agents<'a> {
    pub state: &'a SharedState,
    pub admin: &'a dyn AuthzAdmin,
    pub keychain: &'a dyn SecretSink,
    /// The environment's id.
    pub env: &'a str,
    /// Its name, in the accounts' names and descriptions.
    pub env_name: &'a str,
}

impl Agents<'_> {
    /// Make the environment an account per role of `roles` on `branch`, each in its
    /// role's group, and issue each one a key into the keychain, the environment's local
    /// `WORKGRAPH_<ROLE>_API_KEY` referring to it. A role in `existing` (on the same
    /// branch, its key still set) keeps its account and key. Returns every role's
    /// identity.
    pub async fn provision(
        &self,
        branch: &str,
        roles: &[CatalogRole],
        existing: &[AgentIdentity],
    ) -> std::result::Result<Vec<AgentIdentity>, String> {
        let mut out = Vec::new();
        for role in roles {
            let var = secret_var(&role.name);
            if let Some(known) = existing
                .iter()
                .find(|i| i.role == role.name && i.branch == branch)
                && self
                    .state
                    .var_is_set(self.env, &var)
                    .map_err(|e| e.to_string())?
            {
                out.push(known.clone());
                continue;
            }
            let name = account(&role.name, self.env_name, self.env);
            let group = group(&role.name);
            let description = format!("The {} agent of environment {}", role.name, self.env_name);
            self.admin.ensure_account(&name, &description).await?;
            self.admin.add_to_group(&group, &name).await?;
            let key_id = self.issue(&name, &var).await?;
            out.push(AgentIdentity {
                role: role.name.clone(),
                account: name,
                group,
                key_id: Some(key_id),
                branch: branch.to_owned(),
            });
        }
        Ok(out)
    }

    /// Issue `account` a key into `var`, returning its id; revoked again if the keychain
    /// refuses it, so no key is live that nothing holds.
    async fn issue(&self, account: &str, var: &str) -> std::result::Result<String, String> {
        let issued = self
            .admin
            .issue_key(account, &format!("desktop: {}", self.env_name))
            .await?;
        if let Err(e) = self
            .state
            .store_route_key(self.env, var, &issued.key, self.keychain)
        {
            let _ = self.admin.revoke_key(&issued.id).await;
            return Err(e.to_string());
        }
        Ok(issued.id)
    }

    /// Issue `identity` a new key into the keychain (the same variable), returning the
    /// identity with the new key's id. The caller recreates the running instances'
    /// `secret-server`, then calls [`retire`] with the old id.
    pub async fn rotate(
        &self,
        identity: &AgentIdentity,
    ) -> std::result::Result<AgentIdentity, String> {
        let key_id = self
            .issue(&identity.account, &secret_var(&identity.role))
            .await?;
        Ok(AgentIdentity {
            key_id: Some(key_id),
            ..identity.clone()
        })
    }

    /// Undo `identities`: revoke their keys, take the accounts out of their groups, and
    /// forget the keys. The accounts stay, keyless, so turning the agents on again reuses
    /// them.
    pub async fn deprovision(
        &self,
        identities: &[AgentIdentity],
    ) -> std::result::Result<(), String> {
        for identity in identities {
            retire(self.admin, identity.key_id.as_deref()).await?;
            self.admin
                .remove_from_group(&identity.group, &identity.account)
                .await?;
            self.state
                .forget_route_key(self.env, &secret_var(&identity.role), self.keychain)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Revoke an old key, once nothing uses it.
pub async fn retire(
    admin: &dyn AuthzAdmin,
    key_id: Option<&str>,
) -> std::result::Result<(), String> {
    match key_id {
        Some(id) => admin.revoke_key(id).await,
        None => Ok(()),
    }
}

/// The branch an environment's agents live on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchRef {
    pub project: String,
    /// Its id or name, as the Neon CLI takes it.
    pub branch: String,
}

/// Where environment `env_id`'s agents live: the branch its instances are bound to, else
/// the existing branch its target names (in its project, else the workspace's). A target
/// that makes a new branch per Start has none to provision on yet.
pub fn branch_of<E: olai_stack_runtime::ComposeEngine>(
    runner: &crate::instances::Runner<E>,
    neon: &crate::neon::NeonEnvironments,
    target: Option<&crate::neon::NeonTarget>,
    env_id: &str,
) -> std::result::Result<BranchRef, String> {
    if let Some(binding) = runner.branch_binding(env_id).map_err(|e| e.to_string())? {
        return Ok(BranchRef {
            project: binding.project_id,
            branch: binding.branch,
        });
    }
    match target.map(|t| (&t.project, &t.branch)) {
        Some((project, crate::neon::BranchChoice::Existing { branch })) => {
            let project = match project {
                Some(p) => p.clone(),
                None => {
                    let ws = runner.workspace_of(env_id).map_err(|e| e.to_string())?;
                    neon.project(&ws).map_err(|e| e.to_string())?.0
                }
            };
            Ok(BranchRef {
                project,
                branch: branch.clone(),
            })
        }
        Some((_, crate::neon::BranchChoice::New { .. })) => Err(
            "the agents' accounts live on one branch: pick an existing branch for the \
             environment (a new branch per Start has none yet)"
                .into(),
        ),
        None => Err("pick a Neon branch that runs the workgraph function first".into()),
    }
}

/// The URL `branch`'s `workgraph` function is invoked at.
pub async fn function_url(
    neon: &dyn olai_stack_runtime::neon::NeonControl,
    branch: &BranchRef,
) -> std::result::Result<String, String> {
    neon.functions(&branch.project, &branch.branch)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|f| f.slug == FUNCTION)
        .map(|f| f.invocation_url)
        .ok_or_else(|| format!("the branch runs no `{FUNCTION}` function: deploy it there first"))
}

/// An [`AuthzAdmin`] on `branch`'s main database, where `authz` lives, over a direct
/// connection the Neon CLI hands out.
pub async fn connect_admin(
    neon: &dyn olai_stack_runtime::neon::NeonControl,
    branch: &BranchRef,
) -> std::result::Result<PgAuthzAdmin, String> {
    let connection = neon
        .connection(&branch.project, &branch.branch, None, false)
        .await
        .map_err(|e| e.to_string())?;
    PgAuthzAdmin::connect(connection.url.expose()).await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;

    const PLANNER_KEY: &str =
        "olk_0123456789abcdef_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn catalog() -> RoleCatalog {
        let role = |name: &str, tools: &[&str]| CatalogRole {
            name: name.into(),
            policy: String::new(),
            tools: tools.iter().map(|t| (*t).to_owned()).collect(),
        };
        RoleCatalog {
            roles: vec![
                role("planner", &["plan_read", "add_item", "submit"]),
                role("reviewer", &["plan_read", "annotate"]),
            ],
        }
    }

    #[test]
    fn routes_come_from_the_catalog() {
        let routes = routes_for(&catalog().roles);
        assert_eq!(routes.len(), 2);
        let planner = &routes[0];
        assert_eq!(planner.name, "workgraph-planner");
        assert_eq!(planner.function.as_deref(), Some("workgraph"));
        assert_eq!(planner.path, "/mcp");
        assert_eq!(planner.consumers, [ModuleId("omnigent".into())]);
        assert_eq!(planner.credential.secret_var(), "WORKGRAPH_PLANNER_API_KEY");
        assert_eq!(
            planner.mcp.as_ref().unwrap().tools,
            ["plan_read", "add_item", "submit"]
        );
        // trestle reads them back as written.
        let toml = toml::to_string(&olai_stack_topology::EgressSelection {
            enabled: None,
            routes: routes.clone(),
        })
        .unwrap();
        assert!(
            toml.contains(r#"secret = "WORKGRAPH_PLANNER_API_KEY""#),
            "{toml}"
        );
    }

    #[test]
    fn replacing_keeps_other_routes() {
        let mut selection = Selection::default();
        let mut other = routes_for(&catalog().roles)[0].clone();
        other.name = "tickets".into();
        other.function = None;
        other.upstream = Some("https://tickets.example".into());
        selection.egress.routes.push(other);
        replace_routes(&mut selection, routes_for(&catalog().roles));
        replace_routes(&mut selection, routes_for(&catalog().roles));
        let names: Vec<_> = selection
            .egress
            .routes
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["tickets", "workgraph-planner", "workgraph-reviewer"]
        );
        replace_routes(&mut selection, Vec::new());
        assert_eq!(selection.egress.routes.len(), 1);
    }

    #[test]
    fn the_apps_token_is_never_a_route_credential() {
        for secret in [
            "FACTORY_MCP_APP_TOKENS",
            "FACTORY_MCP_APP_PRINCIPALS",
            "LUMINESCENT_SERVICE_TOKENS",
        ] {
            let mut route = routes_for(&catalog().roles)[0].clone();
            route.credential = InjectedCredential::Bearer {
                secret: secret.into(),
            };
            let err = check_route_credential(&route).unwrap_err();
            assert!(err.contains("approve its own plan"), "{secret}: {err}");
            let mut selection = Selection::default();
            selection.egress.routes.push(route);
            assert!(check_routes(&selection).is_err());
        }
        assert!(check_route_credential(&routes_for(&catalog().roles)[0]).is_ok());
    }

    #[test]
    fn only_a_service_accounts_key_is_taken() {
        assert!(check_key_value(PLANNER_KEY).is_ok());
        for bad in [
            "",
            "wg_desk_0123456789abcdef0123456789abcdef",
            "eyJhbGciOiJFZERTQSJ9.e30.sig",
            "olk_0123_00",
            &PLANNER_KEY.replace("olk_", "olx_"),
            &format!("{PLANNER_KEY}0"),
        ] {
            assert!(check_key_value(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn accounts_fit_authz_names_and_differ_per_environment() {
        let a = account("planner", "Dev Env", "6f1c2a9b-0000-4000");
        assert_eq!(a, "factory-planner-dev-env-6f1c2a9b");
        let b = account("planner", "Dev Env", "aa11bb22-0000");
        assert_ne!(a, b);
        let long = account("executor", &"x".repeat(100), "0123456789");
        assert!(long.len() <= 63 && long.ends_with("-01234567"), "{long}");
        let ok = |s: &str| {
            s.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        };
        assert!(ok(&a) && ok(&long));
        assert_eq!(group("reviewer"), "factory-reviewers");
    }

    #[test]
    fn drift_names_what_the_function_changed() {
        let mut selection = Selection::default();
        replace_routes(&mut selection, routes_for(&catalog().roles));
        assert!(drift(&selection, &catalog()).is_empty());
        let mut changed = catalog();
        changed.roles[0].tools.push("propose".into());
        changed.roles.remove(1);
        let found = drift(&selection, &changed);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("propose") && found[1].contains("no role reviewer"));
    }

    // The store side: the draft and the keys.

    #[derive(Default)]
    struct Keychain(Mutex<BTreeMap<String, String>>);

    impl SecretSink for Keychain {
        fn provider(&self) -> &str {
            "keychain"
        }
        fn set(&self, key: &str, value: &str) -> std::result::Result<(), String> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> std::result::Result<(), String> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        state: SharedState,
        env: String,
    }

    fn fixture(modules: &[&str]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let state = SharedState::open(Some(dir.path())).unwrap();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let env = state.workspace(&ws.id).unwrap().environments.remove(0);
        let mut selection = env.selection.clone();
        let list = selection.0["modules"].as_array_mut().unwrap();
        for m in modules {
            list.push((*m).into());
        }
        if modules.contains(&"omnigent") {
            // Without MLflow, Omnigent takes provider keys.
            selection.0["knob_overrides"]["omnigent"] = serde_json::json!({ "upstream": "keys" });
        }
        state.set_selection(&env.id, selection).unwrap();
        state.commit(&env.id).unwrap();
        Fixture {
            _dir: dir,
            state,
            env: env.id,
        }
    }

    #[test]
    fn routes_need_omnigent_and_land_in_the_draft() {
        let f = fixture(&[]);
        let refused = f
            .state
            .set_workgraph_routes(&f.env, routes_for(&catalog().roles))
            .unwrap_err();
        assert!(refused.to_string().contains("Omnigent"), "{refused}");

        let f = fixture(&["omnigent"]);
        let applied = f
            .state
            .set_workgraph_routes(&f.env, routes_for(&catalog().roles))
            .unwrap();
        // The routes were all the draft changed: committed, and the revision plans.
        assert!(
            applied.committed.is_some() && !applied.needs_commit,
            "{applied:?}"
        );
        let selection = f.state.draft_selection(&f.env).unwrap();
        let views = route_views(&selection, |_| false);
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].role.as_deref(), Some("planner"));
        assert!(!views[0].key_set);
        f.state.set_workgraph_routes(&f.env, Vec::new()).unwrap();
        assert!(
            f.state
                .draft_selection(&f.env)
                .unwrap()
                .egress
                .routes
                .is_empty()
        );
    }

    #[test]
    fn a_key_goes_to_the_keychain_and_a_local_reference() {
        let f = fixture(&["omnigent"]);
        let keychain = Keychain::default();
        let var = secret_var("planner");
        f.state
            .store_route_key(&f.env, &var, PLANNER_KEY, &keychain)
            .unwrap();
        assert!(f.state.var_is_set(&f.env, &var).unwrap());
        let held = keychain.0.lock().unwrap().clone();
        assert_eq!(held.values().collect::<Vec<_>>(), [PLANNER_KEY]);
        // The vars hold the reference only.
        let vars = f.state.vars(&f.env, None).unwrap();
        let entry = vars.local.iter().find(|v| v.key == var).unwrap();
        assert!(entry.secret && !entry.value.contains(PLANNER_KEY));
        // The apps token is refused, whatever it is pasted as.
        let apps = "wg_desk_0123456789abcdef0123456789abcdef";
        assert!(
            f.state
                .store_route_key(&f.env, &var, apps, &keychain)
                .is_err()
        );
        assert!(
            f.state
                .store_route_key(&f.env, "FACTORY_MCP_APP_TOKENS", PLANNER_KEY, &keychain)
                .is_err()
        );
        f.state.forget_route_key(&f.env, &var, &keychain).unwrap();
        assert!(!f.state.var_is_set(&f.env, &var).unwrap());
        assert!(keychain.0.lock().unwrap().is_empty());
    }

    /// Records the calls; keys are numbered.
    #[derive(Default)]
    struct FakeAdmin {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl AuthzAdmin for FakeAdmin {
        async fn ensure_account(&self, name: &str, _: &str) -> std::result::Result<(), String> {
            self.calls.lock().unwrap().push(format!("account {name}"));
            Ok(())
        }
        async fn add_to_group(
            &self,
            group: &str,
            account: &str,
        ) -> std::result::Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("add {group} {account}"));
            Ok(())
        }
        async fn remove_from_group(
            &self,
            group: &str,
            account: &str,
        ) -> std::result::Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("remove {group} {account}"));
            Ok(())
        }
        async fn issue_key(
            &self,
            account: &str,
            _: &str,
        ) -> std::result::Result<IssuedKey, String> {
            let mut calls = self.calls.lock().unwrap();
            calls.push(format!("issue {account}"));
            let n = calls.iter().filter(|c| c.starts_with("issue")).count();
            let id = format!("{n:016x}");
            Ok(IssuedKey {
                key: format!("olk_{id}_{}", format!("{n:x}").repeat(64)[..64].to_owned()),
                id,
            })
        }
        async fn revoke_key(&self, id: &str) -> std::result::Result<(), String> {
            self.calls.lock().unwrap().push(format!("revoke {id}"));
            Ok(())
        }
    }

    #[tokio::test]
    async fn provisioning_rotating_and_undoing() {
        let f = fixture(&["omnigent"]);
        let keychain = Keychain::default();
        let admin = FakeAdmin::default();
        let agents = Agents {
            state: &f.state,
            admin: &admin,
            keychain: &keychain,
            env: &f.env,
            env_name: "dev",
        };
        let roles = catalog().roles;
        let made = agents.provision("br-1", &roles, &[]).await.unwrap();
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].group, "factory-planners");
        assert!(made[0].account.starts_with("factory-planner-dev-"));
        assert!(
            f.state
                .var_is_set(&f.env, "WORKGRAPH_REVIEWER_API_KEY")
                .unwrap()
        );
        assert_eq!(keychain.0.lock().unwrap().len(), 2);
        // The keys went to the keychain only: no identity carries one.
        assert!(!format!("{made:?}").contains("olk_"));

        // Again: nothing new.
        let again = agents.provision("br-1", &roles, &made).await.unwrap();
        assert_eq!(again, made);
        let issued = |a: &FakeAdmin| {
            a.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.starts_with("issue"))
                .count()
        };
        assert_eq!(issued(&admin), 2);

        let before = keychain.0.lock().unwrap().clone();
        let rotated = agents.rotate(&made[0]).await.unwrap();
        assert_ne!(rotated.key_id, made[0].key_id);
        assert_ne!(
            *keychain.0.lock().unwrap(),
            before,
            "the keychain holds the new key"
        );
        retire(&admin, made[0].key_id.as_deref()).await.unwrap();
        assert!(
            admin
                .calls
                .lock()
                .unwrap()
                .contains(&format!("revoke {}", made[0].key_id.as_deref().unwrap()))
        );

        agents
            .deprovision(&[rotated, made[1].clone()])
            .await
            .unwrap();
        assert!(keychain.0.lock().unwrap().is_empty());
        assert!(
            !f.state
                .var_is_set(&f.env, "WORKGRAPH_PLANNER_API_KEY")
                .unwrap()
        );
        assert!(
            admin
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.starts_with("remove factory-reviewers"))
        );
    }
}
