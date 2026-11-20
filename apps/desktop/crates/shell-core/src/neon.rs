//! Neon as where an environment runs (#72): an instance bound to a branch of the
//! workspace's Neon project, as `trestle up --neon-branch` binds one. The revision's
//! function-hosted modules (`neon-function`) then run on the branch, and its local
//! services may use the branch's Postgres and object storage; the binding is the
//! instance's `neon` source, which olai-stack-runtime's [`NeonBranchSource`] resolves at
//! start. The CLI and the app bind the same way, so either sees what the other bound.
//!
//! Each environment has its own target ([`NeonTarget`]): a branch, and optionally a
//! project of its own, so a dev and a prod environment can run side by side on
//! different branches or projects. Without one, the project is the workspace's
//! (`trestle ws set-neon <ws> --project <id>`), else [`PROJECT_VAR`], else
//! [`DEFAULT_PROJECT`]. Branches and functions come from the Neon CLI through
//! olai-stack-runtime's [`NeonControl`], signed in as the CLI is.
//!
//! [`NeonBranchSource`]: olai_stack_runtime::neon::NeonBranchSource

use std::sync::Arc;
use std::time::Duration;

use olai_stack_runtime::neon::{
    CredentialRef, NEON_SOURCE, NeonBinding, NeonControl, NeonError, StorageCredentialRef, bind,
    bind_ai_gateway, create_owned_branch, delete_owned_branch, expires_in, find_branch,
    reset_owned_branch, unbind,
};
use olai_stack_state::{
    Environment, Instance, SecretRef, SourceBinding, Store, Workspace, WorkspaceId,
};
use olai_stack_topology::NeonRequirements;
use serde::{Deserialize, Serialize};

use crate::gateway::{self, GatewayProblem, ModelsApi, NeonGateway, ProjectInfo};
use crate::instances::{RunError, ServiceRoot};
use crate::keys::ServiceKeys;
use crate::toolchain::System;

/// The project the app lists when the workspace names none: the one `neon.ts`
/// deploys to.
pub const DEFAULT_PROJECT: &str = "cool-poetry-28343550";

/// Overrides [`DEFAULT_PROJECT`] (not a workspace's own project).
pub const PROJECT_VAR: &str = "LUMINESCENT_NEON_PROJECT";

/// The keys `trestle ws set-neon` stores the project and the parent branch under.
const PROJECT_KEY: &str = "project_id";
const PARENT_KEY: &str = "parent";

/// How long a branch made for an instance lives unless asked otherwise: the CLI's
/// default (`trestle up --neon-branch new --ttl`).
pub const DEFAULT_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

/// Where the project id came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ProjectSource {
    /// `trestle ws set-neon`.
    Workspace,
    /// [`PROJECT_VAR`].
    Env,
    /// [`DEFAULT_PROJECT`].
    Default,
    /// The environment's own target.
    Environment,
}

/// A branch of the project, as the app lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NeonBranch {
    pub id: String,
    pub name: String,
    /// The project's default branch (production).
    pub default: bool,
    /// Created by trestle (the app or the CLI): only such a branch is reset or deleted.
    pub owned: bool,
    /// When it expires (RFC 3339), if it does.
    pub expires_at: Option<String>,
}

/// The workspace's Neon project and its branches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NeonBranches {
    pub project_id: String,
    pub source: ProjectSource,
    /// The branch a new one is made from (`trestle ws set-neon --parent`), else the
    /// project's default.
    pub parent: Option<String>,
    pub branches: Vec<NeonBranch>,
}

/// Which branch an instance runs on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BranchChoice {
    /// An existing branch, by name or id.
    Existing { branch: String },
    /// A branch made for the instance from the workspace's parent, owned by trestle,
    /// expiring after `ttl_hours` (the CLI's `--neon-branch new --ttl`).
    New { ttl_hours: Option<u32> },
}

/// Where an environment runs on Neon: a branch of a project (the workspace's when
/// `project` is `None`). The environment's header picks it; Start binds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NeonTarget {
    pub project: Option<String>,
    pub branch: BranchChoice,
}

/// A project of the account, as the target picker lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NeonProject {
    pub id: String,
    pub name: String,
    /// The organization's name.
    pub org: String,
}

#[derive(Deserialize)]
struct Named {
    id: String,
    name: String,
}

/// Every project of every organization the Neon CLI is signed in to: `neon orgs
/// list`, then `neon projects list --org-id` for each (`projects list` alone asks
/// which organization).
pub fn list_projects(system: &dyn System) -> Result<Vec<NeonProject>, RunError> {
    let neon = system.locate_neon().ok_or_else(|| RunError::Neon {
        message: "the Neon CLI isn't installed (see Doctor)".into(),
    })?;
    let json = |args: &[&str]| -> Result<Vec<Named>, RunError> {
        let out = system.run(&neon, args).map_err(|e| RunError::Neon {
            message: format!("`neon {}`: {e}", args.join(" ")),
        })?;
        if !out.success {
            return Err(RunError::Neon {
                message: format!("`neon {}`: {}", args.join(" "), out.stderr.trim()),
            });
        }
        serde_json::from_str(&out.stdout).map_err(|e| RunError::Neon {
            message: format!("`neon {}` printed no list: {e}", args.join(" ")),
        })
    };
    let mut projects = Vec::new();
    for org in json(&["orgs", "list", "--output", "json"])? {
        for p in json(&["projects", "list", "--org-id", &org.id, "--output", "json"])? {
            projects.push(NeonProject {
                id: p.id,
                name: p.name,
                org: org.name.clone(),
            });
        }
    }
    projects.sort_by(|a, b| a.org.cmp(&b.org).then_with(|| a.name.cmp(&b.name)));
    Ok(projects)
}

/// Where a binding keeps a credential's secret in the keychain (as `trestle up` does).
fn credential_key(ws: &Workspace, env: &Environment, revision: u32, token_id: &str) -> String {
    format!("neon/{}/{}/r{revision}/{token_id}", ws.name, env.name)
}

/// A project's region and its organization's plan, from the Neon CLI: `neon projects
/// get` (its `region_id` and `org_id`), then `neon orgs list` (each org's `plan`).
pub fn project_info(system: &dyn System, project: &str) -> Result<ProjectInfo, RunError> {
    let neon = system.locate_neon().ok_or_else(|| RunError::Neon {
        message: "the Neon CLI isn't installed (see Doctor)".into(),
    })?;
    let json = |args: &[&str]| -> Result<serde_json::Value, RunError> {
        let out = system.run(&neon, args).map_err(|e| RunError::Neon {
            message: format!("`neon {}`: {e}", args.join(" ")),
        })?;
        if !out.success {
            return Err(RunError::Neon {
                message: format!("`neon {}`: {}", args.join(" "), out.stderr.trim()),
            });
        }
        serde_json::from_str(&out.stdout).map_err(|e| RunError::Neon {
            message: format!("`neon {}` printed no JSON: {e}", args.join(" ")),
        })
    };
    let p = json(&["projects", "get", project, "--output", "json"])?;
    // Some versions wrap it as `{"project": {…}}`.
    let p = p.get("project").cloned().unwrap_or(p);
    let field =
        |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).map(str::to_owned);
    let region = field(&p, "region_id");
    let plan = match field(&p, "org_id") {
        Some(org) => json(&["orgs", "list", "--output", "json"])?
            .as_array()
            .and_then(|orgs| {
                orgs.iter()
                    .find(|o| field(o, "id").as_deref() == Some(&org))
            })
            .and_then(|o| field(o, "plan")),
        None => None,
    };
    Ok(ProjectInfo { region, plan })
}

/// What a revision needs of a Neon branch, as the Start dialog shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NeonNeeds {
    /// It can't start without a branch.
    pub needed: bool,
    /// The functions it runs on the branch, by the key the views address them by
    /// ([`ServiceKeys`]).
    pub functions: Vec<String>,
    /// The databases it uses on the branch.
    pub databases: Vec<String>,
    /// Whether it uses the branch's object storage.
    pub storage: bool,
}

impl NeonNeeds {
    pub fn of(req: &NeonRequirements, keys: &ServiceKeys) -> Self {
        NeonNeeds {
            needed: !req.is_empty(),
            functions: req
                .functions
                .iter()
                .map(|f| keys.function(&f.slug))
                .collect(),
            databases: req.databases.clone(),
            storage: !req.buckets.is_empty() || req.vars.iter().any(|v| v.starts_with("NEON_S3_")),
        }
    }

    /// In a few words, for an error.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.functions.is_empty() {
            parts.push(format!("functions {}", self.functions.join(", ")));
        }
        if !self.databases.is_empty() {
            parts.push(format!("databases {}", self.databases.join(", ")));
        }
        if self.storage {
            parts.push("object storage".to_owned());
        }
        if parts.is_empty() {
            "the branch's variables".to_owned()
        } else {
            parts.join("; ")
        }
    }
}

/// The branch an instance is bound to, as its status shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct BoundBranch {
    pub id: String,
    pub name: String,
    /// What it runs as functions for the instance, by the key the views address
    /// them by ([`ServiceKeys`]).
    pub functions: Vec<String>,
}

/// The instance's Neon binding, if it has one.
pub fn binding_of(instance: &Instance) -> Option<NeonBinding> {
    instance
        .sources
        .iter()
        .find(|s| s.name == NEON_SOURCE)
        .and_then(|s| serde_json::from_value(s.config.clone()).ok())
}

/// The instance's branch, as its status shows it.
pub fn bound_branch(instance: &Instance, keys: &ServiceKeys) -> Option<BoundBranch> {
    binding_of(instance).map(|b| BoundBranch {
        id: b.branch,
        name: b.branch_name,
        functions: b.functions.keys().map(|s| keys.function(s)).collect(),
    })
}

/// Where a binding keeps its storage credential's secret: the CLI's keychain.
pub trait SecretSink: Send + Sync {
    /// The provider name a [`SecretRef`] names it by.
    fn provider(&self) -> &str;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

impl SecretSink for olai_stack_runtime::KeychainProvider {
    fn provider(&self) -> &str {
        "keychain"
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        olai_stack_runtime::KeychainProvider::set(
            self,
            key,
            &olai_stack_runtime::SecretValue::new(value),
        )
        .map_err(|e| e.to_string())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        olai_stack_runtime::KeychainProvider::delete(self, key).map_err(|e| e.to_string())
    }
}

impl From<NeonError> for RunError {
    fn from(e: NeonError) -> Self {
        RunError::Neon {
            message: e.to_string(),
        }
    }
}

/// The workspace's Neon project: its branches, and binding instances to them.
pub struct NeonEnvironments {
    store: Arc<Store>,
    neon: Arc<dyn NeonControl>,
    secrets: Arc<dyn SecretSink>,
    /// [`PROJECT_VAR`], read once.
    env_project: Option<String>,
}

impl NeonEnvironments {
    pub fn new(
        store: Arc<Store>,
        neon: Arc<dyn NeonControl>,
        secrets: Arc<dyn SecretSink>,
        env_project: Option<String>,
    ) -> Self {
        NeonEnvironments {
            store,
            neon,
            secrets,
            env_project: env_project.filter(|p| !p.trim().is_empty()),
        }
    }

    /// The Neon CLI (or a fake) it works through.
    pub fn control(&self) -> Arc<dyn NeonControl> {
        Arc::clone(&self.neon)
    }

    fn workspace(&self, workspace_id: &str) -> Result<Workspace, RunError> {
        Ok(self
            .store
            .workspace_by_id(&WorkspaceId(workspace_id.to_owned()))?)
    }

    fn setting(ws: &Workspace, key: &str) -> Option<String> {
        ws.sources
            .get(NEON_SOURCE)
            .and_then(|c| c.get(key))
            .and_then(|v| v.as_str())
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
    }

    fn project_of(&self, ws: &Workspace) -> (String, ProjectSource) {
        match (Self::setting(ws, PROJECT_KEY), &self.env_project) {
            (Some(p), _) => (p, ProjectSource::Workspace),
            (None, Some(p)) => (p.clone(), ProjectSource::Env),
            (None, None) => (DEFAULT_PROJECT.to_owned(), ProjectSource::Default),
        }
    }

    /// The project the workspace's branches come from, and where it came from.
    pub fn project(&self, workspace_id: &str) -> Result<(String, ProjectSource), RunError> {
        Ok(self.project_of(&self.workspace(workspace_id)?))
    }

    /// A project's branches (the workspace's when `project` is `None`), the default
    /// (production) first.
    pub async fn branches(
        &self,
        workspace_id: &str,
        project: Option<&str>,
    ) -> Result<NeonBranches, RunError> {
        let ws = self.workspace(workspace_id)?;
        let (project_id, source) = match project {
            Some(p) => (p.to_owned(), ProjectSource::Environment),
            None => self.project_of(&ws),
        };
        let mut branches: Vec<NeonBranch> = self
            .neon
            .branches(&project_id)
            .await?
            .into_iter()
            .map(|b| NeonBranch {
                owned: b.is_owned(),
                id: b.id,
                name: b.name,
                default: b.default,
                expires_at: b.expires_at,
            })
            .collect();
        branches.sort_by(|a, b| b.default.cmp(&a.default).then_with(|| a.name.cmp(&b.name)));
        Ok(NeonBranches {
            project_id,
            source,
            parent: Self::setting(&ws, PARENT_KEY),
            branches,
        })
    }

    /// Make the branch `name` now, in `project` (else the workspace's, from its
    /// parent), owned by trestle and expiring after `ttl_hours` (else [`DEFAULT_TTL`]):
    /// what an environment's target can then point at. A branch made from the parent
    /// copies its functions, so it runs what the parent does.
    pub async fn create_branch(
        &self,
        workspace_id: &str,
        project: Option<&str>,
        name: &str,
        ttl_hours: Option<u32>,
    ) -> Result<NeonBranch, RunError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(RunError::Neon {
                message: "a branch needs a name".into(),
            });
        }
        let ws = self.workspace(workspace_id)?;
        let (project_id, parent) = match project {
            Some(p) => (p.to_owned(), None),
            None => (self.project_of(&ws).0, Self::setting(&ws, PARENT_KEY)),
        };
        if self
            .neon
            .branches(&project_id)
            .await?
            .iter()
            .any(|b| b.name == name)
        {
            return Err(RunError::Neon {
                message: format!("the project already has a branch `{name}`"),
            });
        }
        let ttl = ttl_hours.map_or(DEFAULT_TTL, |h| Duration::from_secs(u64::from(h) * 3600));
        let b = create_owned_branch(
            self.neon.as_ref(),
            &project_id,
            name,
            parent.as_deref(),
            Some(expires_in(ttl)),
            None,
        )
        .await?;
        Ok(NeonBranch {
            owned: true,
            id: b.id,
            name: b.name,
            default: b.default,
            expires_at: b.expires_at,
        })
    }

    /// Bind `instance` (revision `revision` of `env` in `ws`) to `target`'s branch, in
    /// its project (else the workspace's), for a plan that needs `req`, as `trestle up
    /// --neon-branch` does: unless
    /// it is bound to it already, any other binding is undone, the branch is checked
    /// for the functions, its missing databases and buckets are created, and a
    /// storage credential is minted when the plan uses object storage (its secret in
    /// the keychain, the binding holds a reference).
    pub async fn bind_instance(
        &self,
        ws: &Workspace,
        env: &Environment,
        instance: &Instance,
        revision: u32,
        target: &NeonTarget,
        req: &NeonRequirements,
    ) -> Result<NeonBinding, RunError> {
        let project = match &target.project {
            Some(p) => p.clone(),
            None => self.project_of(ws).0,
        };
        let branch = match &target.branch {
            BranchChoice::Existing { branch } => branch.clone(),
            BranchChoice::New { ttl_hours } => {
                let ttl =
                    ttl_hours.map_or(DEFAULT_TTL, |h| Duration::from_secs(u64::from(h) * 3600));
                let name = format!("{}-{}-r{revision}", ws.name, env.name);
                create_owned_branch(
                    self.neon.as_ref(),
                    &project,
                    &name,
                    // The workspace's parent is a branch of the workspace's project.
                    target
                        .project
                        .is_none()
                        .then(|| Self::setting(ws, PARENT_KEY))
                        .flatten()
                        .as_deref(),
                    Some(expires_in(ttl)),
                    Some(&instance.id.0),
                )
                .await?
                .id
            }
        };
        if let Some(current) = binding_of(instance) {
            if current.project_id == project
                && (current.branch == branch || current.branch_name == branch)
            {
                return self
                    .ensure_ai_gateway(ws, env, instance, revision, current, req)
                    .await;
            }
            self.unbind_instance(instance).await?;
        }
        let credential_name = format!("trestle-{}", instance.id);
        let (mut binding, credential) =
            bind(self.neon.as_ref(), &project, &branch, req, &credential_name).await?;
        if let Some(credential) = credential {
            let key = credential_key(ws, env, revision, &credential.token_id);
            if let Err(message) = self
                .secrets
                .set(&key, credential.secret_access_key.expose())
            {
                // Don't leave the credential behind on the branch.
                let _ = self
                    .neon
                    .revoke_credential(&binding.project_id, &binding.branch, &credential.token_id)
                    .await;
                return Err(RunError::Neon { message });
            }
            binding.storage_credential = Some(StorageCredentialRef {
                token_id: credential.token_id,
                secret: SecretRef {
                    provider: self.secrets.provider().to_owned(),
                    key,
                },
            });
        }
        if req.ai_gateway
            && let Err(e) = self
                .mint_ai_gateway(ws, env, instance, revision, &mut binding)
                .await
        {
            // Don't leave the storage credential behind either.
            let _ = self.release(&binding).await;
            return Err(e);
        }
        self.record(instance, &binding)?;
        Ok(binding)
    }

    /// For a plan that calls the branch's AI Gateway: give the instance's `binding` a
    /// gateway credential when it has none (it was bound before the environment chose the
    /// gateway), and record it. Otherwise the binding as it is.
    pub async fn ensure_ai_gateway(
        &self,
        ws: &Workspace,
        env: &Environment,
        instance: &Instance,
        revision: u32,
        mut binding: NeonBinding,
        req: &NeonRequirements,
    ) -> Result<NeonBinding, RunError> {
        if !req.ai_gateway || binding.ai_gateway_credential.is_some() {
            return Ok(binding);
        }
        self.mint_ai_gateway(ws, env, instance, revision, &mut binding)
            .await?;
        self.record(instance, &binding)?;
        Ok(binding)
    }

    /// Check the branch's gateway, mint the instance a credential that may call it (its
    /// token in the keychain) and put it on `binding`.
    async fn mint_ai_gateway(
        &self,
        ws: &Workspace,
        env: &Environment,
        instance: &Instance,
        revision: u32,
        binding: &mut NeonBinding,
    ) -> Result<(), RunError> {
        let name = format!("trestle-{}-ai-gateway", instance.id);
        let credential = bind_ai_gateway(self.neon.as_ref(), binding, &name).await?;
        let key = credential_key(ws, env, revision, &credential.token_id);
        if let Err(message) = self.secrets.set(&key, credential.token.expose()) {
            let _ = self
                .neon
                .revoke_credential(&binding.project_id, &binding.branch, &credential.token_id)
                .await;
            return Err(RunError::Neon { message });
        }
        binding.ai_gateway_credential = Some(CredentialRef {
            token_id: credential.token_id,
            secret: SecretRef {
                provider: self.secrets.provider().to_owned(),
                key,
            },
        });
        Ok(())
    }

    /// Write `binding` as the instance's `neon` source.
    fn record(&self, instance: &Instance, binding: &NeonBinding) -> Result<(), RunError> {
        let config = serde_json::to_value(binding).map_err(|e| RunError::Other {
            message: e.to_string(),
        })?;
        let mut sources: Vec<SourceBinding> = instance
            .sources
            .iter()
            .filter(|s| s.name != NEON_SOURCE)
            .cloned()
            .collect();
        sources.push(SourceBinding::new(NEON_SOURCE).with_config(config));
        self.store.set_sources(&instance.id, &sources)?;
        Ok(())
    }

    /// Revoke the binding's credentials (storage, AI Gateway) and forget their secrets.
    async fn release(&self, binding: &NeonBinding) -> Result<(), RunError> {
        unbind(self.neon.as_ref(), binding).await?;
        let credentials = [&binding.storage_credential, &binding.ai_gateway_credential];
        for credential in credentials.into_iter().flatten() {
            self.secrets
                .delete(&credential.secret.key)
                .map_err(|message| RunError::Neon { message })?;
        }
        Ok(())
    }

    /// Undo the instance's binding: revoke its credentials (storage, AI Gateway), forget
    /// their secrets, drop the source.
    pub async fn unbind_instance(&self, instance: &Instance) -> Result<(), RunError> {
        let Some(binding) = binding_of(instance) else {
            return Ok(());
        };
        self.release(&binding).await?;
        let sources: Vec<SourceBinding> = instance
            .sources
            .iter()
            .filter(|s| s.name != NEON_SOURCE)
            .cloned()
            .collect();
        self.store.set_sources(&instance.id, &sources)?;
        Ok(())
    }

    /// The AI Gateway of the branch `name_or_id` of `project` (the workspace's when
    /// `None`), as the Models choice shows it (see [`crate::gateway`]): `info`'s plan
    /// and region first, then the gateway's host, then its models, listed with a
    /// credential called `listing_name` that is revoked again whatever the listing
    /// answers. `seeded` are the model ids the environment's MLflow asks it for.
    #[allow(clippy::too_many_arguments)]
    pub async fn gateway(
        &self,
        workspace_id: &str,
        project: Option<&str>,
        name_or_id: &str,
        info: &ProjectInfo,
        models: &dyn ModelsApi,
        listing_name: &str,
        seeded: &[String],
    ) -> Result<NeonGateway, RunError> {
        let project = match project {
            Some(p) => p.to_owned(),
            None => self.project(workspace_id)?.0,
        };
        let branch = find_branch(self.neon.as_ref(), &project, name_or_id).await?;
        let mut out = NeonGateway {
            console_url: gateway::console_url(&project, &branch.id),
            project_id: project.clone(),
            branch: branch.id.clone(),
            branch_name: branch.name.clone(),
            base_url: None,
            models: Vec::new(),
            locked: 0,
            missing_seeded: Vec::new(),
            problem: None,
        };
        if let Some(problem) = info.problem() {
            out.problem = Some(problem);
            return Ok(out);
        }
        let region = || GatewayProblem::Region {
            region: info.region.clone(),
        };
        let base_url = match self.neon.ai_gateway(&project, &branch.id).await {
            Ok(g) if g.enabled && !g.base_url.is_empty() => g.base_url,
            Ok(_) => {
                out.problem = Some(region());
                return Ok(out);
            }
            Err(NeonError::Failed { stderr, .. }) if gateway::says_unavailable(&stderr) => {
                out.problem = Some(region());
                return Ok(out);
            }
            Err(e) => return Err(e.into()),
        };
        out.base_url = Some(base_url.trim_end_matches('/').to_owned());
        let credential = self
            .neon
            .create_ai_gateway_credential(&project, &branch.id, listing_name)
            .await?;
        let listed = models
            .get_models(&base_url, credential.token.expose())
            .await;
        // Revoked whatever the listing answered: it is never stored.
        let revoked = self
            .neon
            .revoke_credential(&project, &branch.id, &credential.token_id)
            .await;
        let listed = match listed {
            Ok((status, body)) => gateway::interpret(status, &body),
            Err(message) => Err(GatewayProblem::Unreachable { message }),
        };
        match listed {
            Ok(list) => {
                out.missing_seeded = gateway::missing_seeded(seeded, &list.enabled);
                out.locked = list.locked;
                out.models = list.enabled;
            }
            Err(problem) => out.problem = Some(problem),
        }
        revoked?;
        Ok(out)
    }

    /// The URLs of the functions the binding runs, by the key the views address them
    /// by ([`ServiceKeys`]): what the views reach for the modules on the branch.
    pub async fn function_roots(
        &self,
        binding: &NeonBinding,
        keys: &ServiceKeys,
    ) -> Result<Vec<ServiceRoot>, RunError> {
        let mut roots: Vec<ServiceRoot> = self
            .neon
            .functions(&binding.project_id, &binding.branch)
            .await?
            .into_iter()
            .filter(|f| binding.functions.contains_key(&f.slug))
            .map(|f| ServiceRoot {
                module_id: keys.function(&f.slug),
                url: f.invocation_url,
            })
            .collect();
        roots.sort_by(|a, b| a.module_id.cmp(&b.module_id));
        Ok(roots)
    }

    /// Delete the branch `name_or_id`, if trestle created it.
    pub async fn delete_branch(
        &self,
        workspace_id: &str,
        name_or_id: &str,
    ) -> Result<NeonBranch, RunError> {
        let (project, _) = self.project(workspace_id)?;
        let b = delete_owned_branch(self.neon.as_ref(), &project, name_or_id).await?;
        Ok(NeonBranch {
            owned: true,
            id: b.id,
            name: b.name,
            default: b.default,
            expires_at: b.expires_at,
        })
    }

    /// Reset the branch `name_or_id` to its parent, if trestle created it.
    pub async fn reset_branch(&self, workspace_id: &str, name_or_id: &str) -> Result<(), RunError> {
        let (project, _) = self.project(workspace_id)?;
        reset_owned_branch(self.neon.as_ref(), &project, name_or_id).await?;
        Ok(())
    }

    /// The branch `name_or_id` of the workspace's project.
    pub async fn find(&self, workspace_id: &str, name_or_id: &str) -> Result<NeonBranch, RunError> {
        let (project, _) = self.project(workspace_id)?;
        let b = find_branch(self.neon.as_ref(), &project, name_or_id).await?;
        Ok(NeonBranch {
            owned: b.is_owned(),
            id: b.id,
            name: b.name,
            default: b.default,
            expires_at: b.expires_at,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use olai_stack_runtime::neon::FakeNeon;
    use olai_stack_state::Store;

    use super::*;

    /// A keychain in memory.
    #[derive(Default)]
    pub struct MemorySecrets(pub Mutex<BTreeMap<String, String>>);

    impl SecretSink for MemorySecrets {
        fn provider(&self) -> &str {
            "keychain"
        }
        fn set(&self, key: &str, value: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn store_with_workspace() -> (tempfile::TempDir, Arc<Store>, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join(olai_stack_state::DB_FILE)).unwrap());
        let ws = store.create_workspace("local", false).unwrap();
        (dir, store, ws.id.0)
    }

    fn environments(store: Arc<Store>, fake: Arc<FakeNeon>, env: Option<&str>) -> NeonEnvironments {
        NeonEnvironments::new(
            store,
            fake,
            Arc::new(MemorySecrets::default()),
            env.map(Into::into),
        )
    }

    #[tokio::test]
    async fn the_project_is_the_workspaces_then_the_env_then_the_default() {
        let (_dir, store, ws) = store_with_workspace();
        let fake = Arc::new(FakeNeon::new("p-env"));
        let neon = environments(store.clone(), fake.clone(), None);
        assert_eq!(
            neon.project(&ws).unwrap(),
            (DEFAULT_PROJECT.to_owned(), ProjectSource::Default)
        );
        let neon = environments(store.clone(), fake.clone(), Some("p-env"));
        assert_eq!(
            neon.project(&ws).unwrap(),
            ("p-env".to_owned(), ProjectSource::Env)
        );
        store
            .set_workspace_source(
                &WorkspaceId(ws.clone()),
                NEON_SOURCE,
                Some(serde_json::json!({ "project_id": "p-ws", "parent": "staging" })),
            )
            .unwrap();
        assert_eq!(
            neon.project(&ws).unwrap(),
            ("p-ws".to_owned(), ProjectSource::Workspace)
        );
    }

    #[tokio::test]
    async fn branches_list_the_default_first_and_which_trestle_owns() {
        let (_dir, store, ws) = store_with_workspace();
        let fake = Arc::new(FakeNeon::new("p"));
        let neon = environments(store, fake.clone(), Some("p"));
        create_owned_branch(fake.as_ref(), "p", "scratch", None, None, None)
            .await
            .unwrap();
        let listed = neon.branches(&ws, None).await.unwrap();
        assert_eq!(listed.project_id, "p");
        let rows: Vec<(&str, bool, bool)> = listed
            .branches
            .iter()
            .map(|b| (b.name.as_str(), b.default, b.owned))
            .collect();
        assert_eq!(rows, [("main", true, false), ("scratch", false, true)]);
    }

    #[tokio::test]
    async fn only_owned_branches_are_reset_or_deleted() {
        let (_dir, store, ws) = store_with_workspace();
        let fake = Arc::new(FakeNeon::new("p"));
        let neon = environments(store, fake.clone(), Some("p"));
        create_owned_branch(fake.as_ref(), "p", "scratch", None, None, None)
            .await
            .unwrap();
        assert!(matches!(
            neon.delete_branch(&ws, "main").await,
            Err(RunError::Neon { message }) if message.contains("main")
        ));
        neon.reset_branch(&ws, "scratch").await.unwrap();
        let deleted = neon.delete_branch(&ws, "scratch").await.unwrap();
        assert_eq!(deleted.name, "scratch");
        let names: Vec<String> = neon
            .branches(&ws, None)
            .await
            .unwrap()
            .branches
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, ["main"]);
    }

    #[tokio::test]
    async fn a_branch_is_made_now_from_the_parent_owned_and_expiring() {
        let (_dir, store, ws) = store_with_workspace();
        let fake = Arc::new(FakeNeon::new("p"));
        let neon = environments(store.clone(), fake.clone(), Some("p"));
        create_owned_branch(fake.as_ref(), "p", "staging", None, None, None)
            .await
            .unwrap();
        store
            .set_workspace_source(
                &WorkspaceId(ws.clone()),
                NEON_SOURCE,
                Some(serde_json::json!({ "project_id": "p", "parent": "staging" })),
            )
            .unwrap();
        let made = neon
            .create_branch(&ws, None, " demo ", Some(12))
            .await
            .unwrap();
        assert_eq!(made.name, "demo");
        assert!(made.owned && !made.default);
        assert!(made.expires_at.is_some());
        let staging = neon.find(&ws, "staging").await.unwrap();
        let parent = fake
            .branches("p")
            .await
            .unwrap()
            .into_iter()
            .find(|b| b.name == "demo")
            .and_then(|b| b.parent_id);
        assert_eq!(parent.as_deref(), Some(staging.id.as_str()));

        // A name the project has, or none, is refused.
        for name in ["demo", "  "] {
            assert!(matches!(
                neon.create_branch(&ws, None, name, None).await,
                Err(RunError::Neon { .. })
            ));
        }
    }

    #[test]
    fn projects_come_from_every_organization() {
        use std::path::{Path, PathBuf};

        use crate::toolchain::CommandOutput;

        struct Canned;
        impl System for Canned {
            fn which(&self, _: &str) -> Option<PathBuf> {
                None
            }
            fn locate_neon(&self) -> Option<PathBuf> {
                Some("/opt/homebrew/bin/neon".into())
            }
            fn run(&self, _: &Path, args: &[&str]) -> std::io::Result<CommandOutput> {
                let stdout = match args.join(" ").as_str() {
                    "orgs list --output json" => {
                        r#"[{"id":"org-a","name":"open-lakehouse","plan":"scale"},{"id":"org-b","name":"Robert"}]"#
                    }
                    "projects list --org-id org-a --output json" => {
                        r#"[{"id":"cool-poetry-28343550","name":"luminescent-eu","region_id":"aws-eu-central-1"}]"#
                    }
                    "projects list --org-id org-b --output json" => {
                        r#"[{"id":"p-2","name":"scratch"},{"id":"p-1","name":"playground"}]"#
                    }
                    other => panic!("unexpected: {other}"),
                };
                Ok(CommandOutput {
                    success: true,
                    stdout: stdout.into(),
                    stderr: String::new(),
                })
            }
        }
        let projects = list_projects(&Canned).unwrap();
        let rows: Vec<(&str, &str)> = projects
            .iter()
            .map(|p| (p.org.as_str(), p.name.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("Robert", "playground"),
                ("Robert", "scratch"),
                ("open-lakehouse", "luminescent-eu")
            ]
        );
    }

    /// A gateway that answers every listing with `status` and `body`, or fails.
    struct CannedModels(std::result::Result<(u16, &'static [u8]), &'static str>);

    #[async_trait::async_trait]
    impl ModelsApi for CannedModels {
        async fn get_models(
            &self,
            base_url: &str,
            token: &str,
        ) -> std::result::Result<(u16, Vec<u8>), String> {
            assert!(base_url.ends_with(".aws.neon.tech"), "{base_url}");
            assert!(token.starts_with("nt_fake_trestle-models-dev"), "{token}");
            self.0
                .map(|(status, body)| (status, body.to_vec()))
                .map_err(str::to_owned)
        }
    }

    const ENABLED: &[u8] = include_bytes!("../tests/fixtures/neon/v1-models-enabled.json");
    const CREDITS: &[u8] = include_bytes!("../tests/fixtures/neon/v1-models-credits-required.json");

    fn scale() -> ProjectInfo {
        ProjectInfo {
            region: Some("aws-eu-central-1".into()),
            plan: Some("scale".into()),
        }
    }

    async fn status_of(
        fake: &Arc<FakeNeon>,
        info: &ProjectInfo,
        models: CannedModels,
        seeded: &[&str],
    ) -> NeonGateway {
        let (_dir, store, ws) = store_with_workspace();
        let neon = environments(store, fake.clone(), Some("p"));
        let seeded: Vec<String> = seeded.iter().map(|s| (*s).to_owned()).collect();
        neon.gateway(
            &ws,
            None,
            "main",
            info,
            &models,
            "trestle-models-dev",
            &seeded,
        )
        .await
        .unwrap()
    }

    fn minted_and_revoked(fake: &FakeNeon) -> (usize, usize) {
        use olai_stack_runtime::neon::NeonCall;
        let calls = fake.calls();
        (
            calls
                .iter()
                .filter(|c| matches!(c, NeonCall::CreateCredential(..)))
                .count(),
            calls
                .iter()
                .filter(|c| matches!(c, NeonCall::RevokeCredential(..)))
                .count(),
        )
    }

    #[tokio::test]
    async fn the_gateway_lists_its_enabled_models_and_revokes_the_listing_credential() {
        let fake = Arc::new(FakeNeon::new("p"));
        fake.set_ai_gateway("main", true);
        let status = status_of(
            &fake,
            &scale(),
            CannedModels(Ok((200, ENABLED))),
            &["claude-opus-5-5", "claude-opus-9"],
        )
        .await;
        assert_eq!(status.problem, None);
        assert!(status.models.iter().any(|m| m.id == "claude-sonnet-5-5"));
        assert_eq!(status.missing_seeded, ["claude-opus-9"]);
        assert_eq!(status.branch_name, "main");
        assert!(status.base_url.unwrap().ends_with(".aws.neon.tech"));
        assert_eq!(
            status.console_url,
            format!(
                "https://console.neon.tech/app/projects/p/branches/{}/ai-gateway",
                status.branch
            )
        );
        assert_eq!(minted_and_revoked(&fake), (1, 1));
        assert!(fake.credentials("main").is_empty());
    }

    #[tokio::test]
    async fn no_credits_and_a_failed_listing_are_problems_and_still_revoke() {
        let fake = Arc::new(FakeNeon::new("p"));
        fake.set_ai_gateway("main", true);
        let status = status_of(&fake, &scale(), CannedModels(Ok((200, CREDITS))), &[]).await;
        assert!(
            matches!(&status.problem, Some(GatewayProblem::Credits { recovery_url: Some(u) }) if u.contains("billing")),
            "{:?}",
            status.problem
        );
        assert!(status.models.is_empty());
        let status = status_of(
            &fake,
            &scale(),
            CannedModels(Err("connection refused")),
            &[],
        )
        .await;
        assert_eq!(
            status.problem,
            Some(GatewayProblem::Unreachable {
                message: "connection refused".into()
            })
        );
        assert_eq!(minted_and_revoked(&fake), (2, 2));
        assert!(fake.credentials("main").is_empty());
    }

    #[tokio::test]
    async fn a_free_plan_or_a_region_without_the_gateway_mints_nothing() {
        let fake = Arc::new(FakeNeon::new("p"));
        fake.set_ai_gateway("main", true);
        let free = ProjectInfo {
            plan: Some("free".into()),
            ..scale()
        };
        let status = status_of(&fake, &free, CannedModels(Ok((200, ENABLED))), &[]).await;
        assert_eq!(status.problem, Some(GatewayProblem::PaidPlan));
        let azure = ProjectInfo {
            region: Some("azure-eastus2".into()),
            ..scale()
        };
        let status = status_of(&fake, &azure, CannedModels(Ok((200, ENABLED))), &[]).await;
        assert!(matches!(
            status.problem,
            Some(GatewayProblem::Region { .. })
        ));
        // The fake's disabled gateway (the real API's 404 is `says_unavailable`).
        fake.set_ai_gateway("main", false);
        let status = status_of(&fake, &scale(), CannedModels(Ok((200, ENABLED))), &[]).await;
        assert_eq!(
            status.problem,
            Some(GatewayProblem::Region {
                region: Some("aws-eu-central-1".into())
            })
        );
        assert_eq!(minted_and_revoked(&fake), (0, 0));
    }

    #[test]
    fn project_info_reads_the_region_and_the_org_s_plan() {
        use std::path::{Path, PathBuf};

        use crate::toolchain::CommandOutput;

        struct Canned;
        impl System for Canned {
            fn which(&self, _: &str) -> Option<PathBuf> {
                None
            }
            fn locate_neon(&self) -> Option<PathBuf> {
                Some("/opt/homebrew/bin/neon".into())
            }
            fn run(&self, _: &Path, args: &[&str]) -> std::io::Result<CommandOutput> {
                // The shapes `neon` 9.3 prints (2026-11-20), trimmed.
                let stdout = match args.join(" ").as_str() {
                    "projects get p-1 --output json" => {
                        r#"{"id":"p-1","name":"luminescent-eu","region_id":"aws-eu-central-1","org_id":"org-a"}"#
                    }
                    "orgs list --output json" => {
                        r#"[{"id":"org-a","name":"open-lakehouse","plan":"scale"},{"id":"org-b","name":"Me","plan":"free"}]"#
                    }
                    other => panic!("unexpected: {other}"),
                };
                Ok(CommandOutput {
                    success: true,
                    stdout: stdout.into(),
                    stderr: String::new(),
                })
            }
        }
        assert_eq!(project_info(&Canned, "p-1").unwrap(), scale());
    }

    #[tokio::test]
    async fn the_http_listing_sends_the_bearer_to_v1_models() {
        use axum::http::{HeaderMap, StatusCode};

        async fn models(headers: HeaderMap) -> (StatusCode, &'static [u8]) {
            match headers.get("authorization").and_then(|v| v.to_str().ok()) {
                Some("Bearer nt_test") => (StatusCode::OK, ENABLED),
                _ => (
                    StatusCode::UNAUTHORIZED,
                    include_bytes!("../tests/fixtures/neon/v1-models-unauthorized.json"),
                ),
            }
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let app = axum::Router::new().route("/v1/models", axum::routing::get(models));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = gateway::HttpModels::default();
        let (status, body) = http.get_models(&base, "nt_test").await.unwrap();
        assert_eq!(status, 200);
        assert!(
            !gateway::interpret(status, &body)
                .unwrap()
                .enabled
                .is_empty()
        );
        let (status, body) = http.get_models(&base, "nt_wrong").await.unwrap();
        assert_eq!(status, 401);
        assert!(matches!(
            gateway::interpret(status, &body),
            Err(GatewayProblem::Unreachable { .. })
        ));
    }

    #[test]
    fn needs_read_as_the_dialog_shows_them() {
        let none = NeonNeeds::of(&NeonRequirements::default(), &ServiceKeys::default());
        assert!(!none.needed);
        assert_eq!(none.describe(), "the branch's variables");
    }
}
