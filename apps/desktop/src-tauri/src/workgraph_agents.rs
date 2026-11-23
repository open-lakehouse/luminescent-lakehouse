//! The Configure tab's "Work graph agents": an environment's egress routes to its
//! branch's `workgraph` function, one per role of the function's catalog, and the
//! service accounts and keys behind them (`shell_core::workgraph_agents`).
//!
//! No command returns a key. The keys go from the branch's `authz` (or a paste) straight
//! into the keychain; the views say only whether each route's key is set.

use olai_stack_runtime::KeychainProvider;
use serde::Serialize;
use shell_core::instances::{KEYCHAIN_SERVICE, RunError};
use shell_core::workgraph_agents::{
    self as agents, AgentIdentity, BranchRef, CatalogRole, RoleCatalog, RouteView, RoutesApplied,
};
use tauri::State;

use crate::AppState;
use crate::run::SharedSettings;

fn other(message: impl std::fmt::Display) -> RunError {
    RunError::Other {
        message: message.to_string(),
    }
}

/// An environment's egress routes and its work-graph agents.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentsView {
    /// Every egress route of the draft, whether this section made it or not.
    pub routes: Vec<RouteView>,
    /// The agents' service accounts on the branch.
    pub identities: Vec<AgentIdentity>,
    /// The draft runs Omnigent, which calls the routes.
    pub omnigent: bool,
    /// The branch the agents live on, or why there is none.
    pub branch: Option<String>,
    pub branch_error: Option<String>,
    /// The roles the branch's function grants, or why they couldn't be read.
    pub catalog: Option<RoleCatalog>,
    pub catalog_error: Option<String>,
    /// Where the routes and the function's roles disagree.
    pub drift: Vec<String>,
}

/// What enabling, disabling or a key change did.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentsApplied {
    pub view: AgentsView,
    /// The routes' revision, when they changed.
    pub routes: Option<RoutesApplied>,
    /// The running instances whose `secret-server` was recreated with a new key.
    pub recreated: Vec<String>,
}

fn keychain() -> KeychainProvider {
    KeychainProvider::new(KEYCHAIN_SERVICE)
}

fn env_name(state: &AppState, env: &str) -> Result<String, RunError> {
    let store = state.store.clone().map_err(other)?;
    store.environment_name(env).map_err(other)
}

fn branch(state: &AppState, settings: &SharedSettings, env: &str) -> Result<BranchRef, RunError> {
    let runner = state.runner.clone()?;
    let neon = state.neon.clone()?;
    let target = settings.get().neon_targets.get(env).cloned();
    agents::branch_of(&runner, &neon, target.as_ref(), env)
        .map_err(|message| RunError::Neon { message })
}

async fn catalog(state: &AppState, branch: &BranchRef) -> Result<RoleCatalog, RunError> {
    let neon = state.neon.clone()?;
    let url = agents::function_url(neon.control().as_ref(), branch)
        .await
        .map_err(|message| RunError::Neon { message })?;
    agents::fetch_roles(&agents::http_client(), &url)
        .await
        .map_err(other)
}

async fn view(
    state: &AppState,
    settings: &SharedSettings,
    env: &str,
) -> Result<AgentsView, RunError> {
    let store = state.store.clone().map_err(other)?;
    let selection = store.draft_selection(env).map_err(other)?;
    let is_set = |var: &str| store.var_is_set(env, var).unwrap_or(false);
    let routes = agents::route_views(&selection, is_set);
    let omnigent = selection.modules.iter().any(|m| m.0 == agents::CONSUMER);
    let (branch, branch_error, catalog, catalog_error) = match branch(state, settings, env) {
        Err(e) => (None, Some(e.to_string()), None, None),
        Ok(b) => {
            let name = Some(b.branch.clone());
            match catalog(state, &b).await {
                Ok(c) => (name, None, Some(c), None),
                Err(e) => (name, None, None, Some(e.to_string())),
            }
        }
    };
    let drift = catalog
        .as_ref()
        .map(|c| agents::drift(&selection, c))
        .unwrap_or_default();
    Ok(AgentsView {
        routes,
        identities: settings
            .get()
            .workgraph_agents
            .get(env)
            .cloned()
            .unwrap_or_default(),
        omnigent,
        branch,
        branch_error,
        catalog,
        catalog_error,
        drift,
    })
}

fn remember(
    settings: &SharedSettings,
    env: &str,
    identities: Vec<AgentIdentity>,
) -> Result<(), RunError> {
    settings
        .update(|s| {
            if identities.is_empty() {
                s.workgraph_agents.remove(env);
            } else {
                s.workgraph_agents.insert(env.to_owned(), identities);
            }
        })
        .map(|_| ())
        .map_err(other)
}

/// The environment's egress routes and work-graph agents, with the roles its branch's
/// function grants.
#[tauri::command]
#[specta::specta]
pub async fn agents_get(
    state: State<'_, AppState>,
    settings: State<'_, SharedSettings>,
    environment: String,
) -> Result<AgentsView, RunError> {
    view(&state, &settings, &environment).await
}

/// Give the environment an agent for each of `roles` (of its branch's function's
/// catalog): a service account in the role's group and a key in the keychain, and a
/// route each into the draft, committed when the routes are all it changes. A role no
/// longer chosen loses its route, key and group membership.
#[tauri::command]
#[specta::specta]
pub async fn agents_enable(
    state: State<'_, AppState>,
    settings: State<'_, SharedSettings>,
    environment: String,
    roles: Vec<String>,
) -> Result<AgentsApplied, RunError> {
    let env = environment.as_str();
    let store = state.store.clone().map_err(other)?;
    if !store
        .draft_selection(env)
        .map_err(other)?
        .modules
        .iter()
        .any(|m| m.0 == agents::CONSUMER)
    {
        return Err(other(
            "the agents reach the work graph through Omnigent: add Omnigent first",
        ));
    }
    let branch = branch(&state, &settings, env)?;
    let catalog = catalog(&state, &branch).await?;
    let mut chosen: Vec<CatalogRole> = Vec::new();
    for role in &roles {
        let found = catalog
            .roles
            .iter()
            .find(|r| &r.name == role)
            .ok_or_else(|| other(format!("the branch's function grants no role {role}")))?;
        chosen.push(found.clone());
    }
    let name = env_name(&state, env)?;
    let neon = state.neon.clone()?;
    let admin = agents::connect_admin(neon.control().as_ref(), &branch)
        .await
        .map_err(|message| RunError::Neon { message })?;
    let keychain = keychain();
    let existing = settings
        .get()
        .workgraph_agents
        .get(env)
        .cloned()
        .unwrap_or_default();
    let (keep, drop): (Vec<_>, Vec<_>) = existing
        .into_iter()
        .partition(|i| roles.contains(&i.role) && i.branch == branch.branch);
    let result = async {
        let agents = agents::Agents {
            state: &store,
            admin: &admin,
            keychain: &keychain,
            env,
            env_name: &name,
        };
        agents.deprovision(&drop).await?;
        agents.provision(&branch.branch, &chosen, &keep).await
    }
    .await;
    admin.close().await;
    let identities = result.map_err(other)?;
    remember(&settings, env, identities)?;
    let applied = store
        .set_workgraph_routes(env, agents::routes_for(&chosen))
        .map_err(other)?;
    Ok(AgentsApplied {
        view: view(&state, &settings, env).await?,
        routes: Some(applied),
        recreated: Vec::new(),
    })
}

/// Take the environment's work-graph agents away: their routes out of the draft, their
/// keys revoked and forgotten, their accounts out of the roles' groups.
#[tauri::command]
#[specta::specta]
pub async fn agents_disable(
    state: State<'_, AppState>,
    settings: State<'_, SharedSettings>,
    environment: String,
) -> Result<AgentsApplied, RunError> {
    let env = environment.as_str();
    let store = state.store.clone().map_err(other)?;
    let identities = settings
        .get()
        .workgraph_agents
        .get(env)
        .cloned()
        .unwrap_or_default();
    if !identities.is_empty() {
        let branch = branch(&state, &settings, env)?;
        let neon = state.neon.clone()?;
        let admin = agents::connect_admin(neon.control().as_ref(), &branch)
            .await
            .map_err(|message| RunError::Neon { message })?;
        let name = env_name(&state, env)?;
        let keychain = keychain();
        let result = agents::Agents {
            state: &store,
            admin: &admin,
            keychain: &keychain,
            env,
            env_name: &name,
        }
        .deprovision(&identities)
        .await;
        admin.close().await;
        result.map_err(other)?;
    }
    remember(&settings, env, Vec::new())?;
    let applied = store.set_workgraph_routes(env, Vec::new()).map_err(other)?;
    Ok(AgentsApplied {
        view: view(&state, &settings, env).await?,
        routes: Some(applied),
        recreated: Vec::new(),
    })
}

/// Rotate `role`'s key: issue a new one into the keychain, recreate the running
/// instances' `secret-server` with it (the egress gateway keeps running), then revoke the
/// old one.
#[tauri::command]
#[specta::specta]
pub async fn agents_rotate(
    state: State<'_, AppState>,
    settings: State<'_, SharedSettings>,
    environment: String,
    role: String,
) -> Result<AgentsApplied, RunError> {
    let env = environment.as_str();
    let store = state.store.clone().map_err(other)?;
    let mut identities = settings
        .get()
        .workgraph_agents
        .get(env)
        .cloned()
        .unwrap_or_default();
    let index = identities
        .iter()
        .position(|i| i.role == role)
        .ok_or_else(|| other(format!("the environment has no {role} agent")))?;
    let old = identities[index].clone();
    let branch = branch(&state, &settings, env)?;
    let neon = state.neon.clone()?;
    let admin = agents::connect_admin(neon.control().as_ref(), &branch)
        .await
        .map_err(|message| RunError::Neon { message })?;
    let name = env_name(&state, env)?;
    let keychain = keychain();
    let rotated = agents::Agents {
        state: &store,
        admin: &admin,
        keychain: &keychain,
        env,
        env_name: &name,
    }
    .rotate(&old)
    .await;
    let rotated = match rotated {
        Ok(r) => r,
        Err(e) => {
            admin.close().await;
            return Err(other(e));
        }
    };
    identities[index] = rotated;
    remember(&settings, env, identities)?;
    // The old key stays live until the secret server serves the new one.
    let recreated = match state.runner.clone()?.recreate_secret_server(env).await {
        Ok(r) => r,
        Err(e) => {
            admin.close().await;
            return Err(e);
        }
    };
    let retired = agents::retire(&admin, old.key_id.as_deref()).await;
    admin.close().await;
    retired.map_err(other)?;
    Ok(AgentsApplied {
        view: view(&state, &settings, env).await?,
        routes: None,
        recreated,
    })
}

/// Set `role`'s key to `value`, a service account's `olk_` key made elsewhere (pasted
/// once; it is never shown again), and hand it to the running instances. An earlier key
/// the app issued is revoked once the secret server serves the new one.
#[tauri::command]
#[specta::specta]
pub async fn agents_set_key(
    state: State<'_, AppState>,
    settings: State<'_, SharedSettings>,
    environment: String,
    role: String,
    value: String,
) -> Result<AgentsApplied, RunError> {
    let env = environment.as_str();
    let store = state.store.clone().map_err(other)?;
    store
        .store_route_key(env, &agents::secret_var(&role), value.trim(), &keychain())
        .map_err(other)?;
    drop(value);
    let recreated = state.runner.clone()?.recreate_secret_server(env).await?;
    let mut identities = settings
        .get()
        .workgraph_agents
        .get(env)
        .cloned()
        .unwrap_or_default();
    if let Some(identity) = identities.iter_mut().find(|i| i.role == role)
        && let Some(old) = identity.key_id.take()
    {
        // Best effort: the branch may be gone, and the key replaced either way.
        if let Ok(branch) = branch(&state, &settings, env) {
            let neon = state.neon.clone()?;
            if let Ok(admin) = agents::connect_admin(neon.control().as_ref(), &branch).await {
                if let Err(e) = agents::retire(&admin, Some(&old)).await {
                    eprintln!("revoking the {role} agent's earlier key: {e}");
                }
                admin.close().await;
            }
        }
    }
    remember(&settings, env, identities)?;
    Ok(AgentsApplied {
        view: view(&state, &settings, env).await?,
        routes: None,
        recreated,
    })
}
