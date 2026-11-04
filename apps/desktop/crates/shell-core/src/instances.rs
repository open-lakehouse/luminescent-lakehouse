//! Running environments: an environment's head revision as a Docker Compose
//! instance, through trestle's `olai-stack-runtime`, as `trestle up` / `trestle down`
//! run them. The app and the CLI share the instances (the store records them and
//! their slots), so either can stop what the other started.
//!
//! What runs comes from Docker (`compose ps`), as `trestle ps` / `trestle health`
//! report it; the store only records what is wanted. [`Runner::changes`] follows
//! Docker's events for every instance, so the UI refreshes when a container starts,
//! dies or changes health, whoever started it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt};
use olai_stack_runtime::neon::{NeonBranchSource, NeonCli};
use olai_stack_runtime::{
    ComposeEngine, DockerCli, EnvProvider, EventAction, EventFilter, Health, InstanceError,
    Instances, KeychainProvider, ReconcileOptions, SecretProviders, ServiceStatus, State,
    instance_project, parse_instance_project, remove_mesh_if_idle,
};
use olai_stack_state::{Desired, EnvironmentId, Instance, InstanceId, Store, WorkspaceId};
use olai_stack_topology::manifest::EnvManifest;
use olai_stack_topology::projection::plan_result_for_manifest;
use olai_stack_topology::{Catalog, baseline_catalog};
use serde::Serialize;

/// How long `compose up --wait` may wait for the services to be healthy.
pub const START_TIMEOUT: Duration = Duration::from_secs(300);

/// The keychain service the CLI keeps secrets under (`trestle var set --secret`).
pub const KEYCHAIN_SERVICE: &str = "trestle";

/// One container of a running instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ServiceRow {
    pub service: String,
    /// `running`, `exited`, `restarting`, …
    pub state: String,
    /// `none` (no healthcheck), `starting`, `healthy` or `unhealthy`.
    pub health: String,
    /// Docker's own status line (`Up 2 minutes (healthy)`).
    pub status: String,
    /// The exit code, for an exited container: a one-shot job (certificates, a
    /// migration) exits 0 when it is done.
    pub exit_code: i32,
    /// Published ports, `host→container`.
    pub ports: Vec<String>,
    /// Running, and healthy if it has a healthcheck.
    pub ready: bool,
}

/// An instance and what Docker says about it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InstanceStatus {
    pub id: String,
    pub revision: u32,
    /// What the store says is wanted: `running` or `stopped`.
    pub desired: String,
    pub slot: Option<u16>,
    /// The compose project, `<workspace>_<env>_r<n>`.
    pub project: String,
    /// Where the gateway listens, once the instance has a slot.
    pub gateway_url: Option<String>,
    pub services: Vec<ServiceRow>,
}

/// An environment's instances.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRun {
    pub id: String,
    pub name: String,
    /// The newest revision, what Start runs.
    pub head: Option<u32>,
    pub instances: Vec<InstanceStatus>,
}

/// A workspace's environments and their instances, for the Run view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRun {
    pub environments: Vec<EnvironmentRun>,
    /// Docker couldn't be asked (not installed, the daemon down): the instances'
    /// containers are unknown, not absent.
    pub engine_problem: Option<String>,
}

/// One container of an instance started, stopped, died or changed health: Docker's
/// event, from any process's instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InstanceChange {
    /// The compose project, `<workspace>_<env>_r<n>`.
    pub project: String,
    pub workspace: String,
    pub service: String,
    /// `create`, `start`, `stop`, `kill`, `die`, `destroy`, `health:<state>`, or
    /// Docker's own word.
    pub action: String,
}

fn action_word(action: &EventAction) -> String {
    match action {
        EventAction::Create => "create".into(),
        EventAction::Start => "start".into(),
        EventAction::Stop => "stop".into(),
        EventAction::Kill => "kill".into(),
        EventAction::Die { .. } => "die".into(),
        EventAction::Destroy => "destroy".into(),
        EventAction::Health(h) => format!("health:{}", health_word(h)),
        EventAction::Other(other) => other.clone(),
    }
}

/// An instance, as the Run view names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InstanceLabel {
    pub id: String,
    /// `<workspace>/<env> #<revision>`.
    pub label: String,
}

/// What matching Docker against the store found at launch ([`Runner::reconcile`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ReconcileReport {
    /// Running, and now recorded as wanted running.
    pub adopted: Vec<InstanceLabel>,
    /// Wanted running, but gone from Docker: now recorded as stopped.
    pub stale: Vec<InstanceLabel>,
    /// Instance projects in Docker the store doesn't know (left alone).
    pub orphans: Vec<String>,
}

impl ReconcileReport {
    /// Nothing to tell.
    pub fn is_quiet(&self) -> bool {
        self.stale.is_empty() && self.orphans.is_empty()
    }
}

/// Where one module of a running instance is reached: its gateway root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ServiceRoot {
    pub module_id: String,
    pub url: String,
}

/// The environment the app currently shows, whose services the views reach: a
/// running instance, or a Neon branch's functions ([`crate::neon`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CurrentEnvironment {
    Instance {
        instance_id: String,
        workspace_id: String,
        workspace: String,
        environment_id: String,
        environment: String,
        revision: u32,
    },
    Neon {
        project_id: String,
        branch_id: String,
        branch: String,
        workspace_id: String,
        workspace: String,
    },
}

impl CurrentEnvironment {
    /// The key the UI remounts on: one per instance or branch.
    pub fn key(&self) -> String {
        match self {
            CurrentEnvironment::Instance { instance_id, .. } => format!("instance:{instance_id}"),
            CurrentEnvironment::Neon { branch_id, .. } => format!("neon:{branch_id}"),
        }
    }

    /// Whether it is an instance of environment `environment_id`.
    pub fn is_instance_of(&self, environment_id: &str) -> bool {
        matches!(self, CurrentEnvironment::Instance { environment_id: e, .. } if e == environment_id)
    }
}

/// Why starting or stopping failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RunError {
    /// Start runs the head revision; there is none yet.
    #[error("{env} has no revision yet: commit one to start it")]
    NoRevision { env: String },
    /// Docker isn't there or the daemon isn't running.
    #[error("{message}")]
    Unavailable { message: String },
    /// Host ports the instance needs are taken.
    #[error("ports in use: {}", ports.join(", "))]
    PortsBusy { ports: Vec<String> },
    /// `docker compose` failed; `output` is what it printed.
    #[error("{message}")]
    Compose { message: String, output: String },
    /// The Neon CLI isn't there, isn't signed in, or a call failed.
    #[error("{message}")]
    Neon { message: String },
    #[error("{message}")]
    Other { message: String },
}

impl From<InstanceError> for RunError {
    fn from(e: InstanceError) -> Self {
        match e {
            InstanceError::Engine(engine) => engine_error(engine),
            InstanceError::PortsBusy(busy) => RunError::PortsBusy {
                ports: busy
                    .iter()
                    .map(|b| format!("{} ({})", b.port, b.owner))
                    .collect(),
            },
            other => RunError::Other {
                message: other.to_string(),
            },
        }
    }
}

impl From<olai_stack_state::Error> for RunError {
    fn from(e: olai_stack_state::Error) -> Self {
        RunError::Other {
            message: e.to_string(),
        }
    }
}

fn engine_error(e: olai_stack_runtime::Error) -> RunError {
    if e.is_unavailable() {
        return RunError::Unavailable {
            message: e.to_string(),
        };
    }
    match e {
        olai_stack_runtime::Error::Failed {
            command, stderr, ..
        } => RunError::Compose {
            message: format!("`{command}` failed"),
            output: stderr,
        },
        other => RunError::Other {
            message: other.to_string(),
        },
    }
}

type Result<T> = std::result::Result<T, RunError>;

fn health_word(health: &Health) -> &'static str {
    match health {
        Health::None => "none",
        Health::Starting => "starting",
        Health::Healthy => "healthy",
        Health::Unhealthy => "unhealthy",
    }
}

fn state_word(state: &State) -> String {
    match state {
        State::Created => "created".into(),
        State::Running => "running".into(),
        State::Paused => "paused".into(),
        State::Restarting => "restarting".into(),
        State::Removing => "removing".into(),
        State::Exited => "exited".into(),
        State::Dead => "dead".into(),
        State::Other(s) => s.clone(),
    }
}

fn service_row(s: &ServiceStatus) -> ServiceRow {
    ServiceRow {
        service: s.service.clone(),
        state: state_word(&s.state),
        health: health_word(&s.health).into(),
        status: s.status.clone(),
        exit_code: s.exit_code,
        ports: s
            .published
            .iter()
            .filter(|p| p.host_port != 0)
            .map(|p| format!("{}→{}", p.host_port, p.container_port))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        ready: s.is_ready(),
    }
}

/// Starts, stops and reports instances, on engine `E` (Docker; a fake in tests).
pub struct Runner<E> {
    store: Arc<Store>,
    runtime: Instances<E>,
}

impl Runner<DockerCli> {
    /// On the `docker` on `PATH`, with secrets from the process environment and the
    /// keychain (as the CLI resolves them), and Neon branch overlays when the Neon CLI
    /// is installed.
    pub fn docker(store: Arc<Store>, home: &Path) -> Self {
        let providers = SecretProviders::new()
            .with(EnvProvider)
            .with(KeychainProvider::new(KEYCHAIN_SERVICE));
        let mut runtime = Instances::new(Arc::clone(&store), DockerCli::new(), home)
            .with_providers(providers.clone())
            .with_wait_timeout(START_TIMEOUT);
        if let Ok(neon) = NeonCli::new() {
            runtime = runtime.with_source(NeonBranchSource::new(Arc::new(neon), providers));
        }
        Runner { store, runtime }
    }
}

impl<E: ComposeEngine> Runner<E> {
    /// On `engine`, with no secret providers beyond the process environment and no
    /// host-port probe (tests).
    pub fn with_engine(store: Arc<Store>, engine: E, home: &Path) -> Self {
        let runtime = Instances::new(Arc::clone(&store), engine, home)
            .without_port_probe()
            .with_wait_timeout(START_TIMEOUT);
        Runner { store, runtime }
    }

    fn env_id(id: &str) -> EnvironmentId {
        EnvironmentId(id.to_owned())
    }

    /// Start the environment's head revision: reuse its instance (the one wanted
    /// running, else any), or create one, then lease a slot, render and `up` it,
    /// waiting for health. A failed `up` leaves it wanted running, as `trestle up`
    /// does: what came up stays to look at, and [`stop`](Self::stop) cleans up.
    ///
    /// Side by side is the default: other instances keep running. With
    /// `one_at_a_time`, the workspace's other instances are stopped first (data
    /// volumes kept), so only this one runs.
    pub async fn start(&self, env_id: &str, one_at_a_time: bool) -> Result<InstanceStatus> {
        let env = self.store.environment(&Self::env_id(env_id))?;
        let head = env.head.ok_or_else(|| RunError::NoRevision {
            env: env.name.clone(),
        })?;
        let existing = self
            .store
            .instances_of(&env.id)?
            .into_iter()
            .filter(|i| i.revision == head)
            .min_by_key(|i| i.desired != Desired::Running);
        let instance = match existing {
            Some(instance) => instance,
            None => self.store.create_instance(&env.id, head)?,
        };
        if one_at_a_time {
            for other in self.live_instances(&env.workspace).await? {
                if other != instance.id {
                    self.stop_instance(&other.0).await?;
                }
            }
        }
        self.runtime.start(&instance.id).await?;
        self.instance_status(&self.store.instance(&instance.id)?, true)
            .await
    }

    /// Stop the environment's instances that are wanted running or still have
    /// containers (`compose down`, data volumes kept), then drop the workspace's mesh
    /// network if no other instance uses it.
    pub async fn stop(&self, env_id: &str) -> Result<()> {
        let env = self.store.environment(&Self::env_id(env_id))?;
        let live = self.live_instances(&env.workspace).await?;
        for instance in self.store.instances_of(&env.id)? {
            if live.contains(&instance.id) {
                self.runtime.stop(&instance.id, false).await?;
            }
        }
        self.drop_idle_mesh(&env.workspace).await
    }

    /// Stop one instance (data volumes kept), then drop the workspace's mesh network
    /// if no other instance uses it.
    pub async fn stop_instance(&self, instance_id: &str) -> Result<()> {
        let instance = self.store.instance(&InstanceId(instance_id.to_owned()))?;
        let env = self.store.environment(&instance.environment)?;
        self.runtime.stop(&instance.id, false).await?;
        self.drop_idle_mesh(&env.workspace).await
    }

    /// The workspace's instances that are wanted running or still have containers.
    async fn live_instances(&self, workspace: &WorkspaceId) -> Result<Vec<InstanceId>> {
        let ws = self.store.workspace_by_id(workspace)?;
        let projects = self.running_projects().await?;
        let mut live = Vec::new();
        for env in self.store.environments(workspace)? {
            for instance in self.store.instances_of(&env.id)? {
                let project = instance_project(&ws.name, &env.name, instance.revision);
                if instance.desired == Desired::Running || projects.contains(&project) {
                    live.push(instance.id);
                }
            }
        }
        Ok(live)
    }

    async fn drop_idle_mesh(&self, workspace: &WorkspaceId) -> Result<()> {
        let ws = self.store.workspace_by_id(workspace)?;
        if ws.mesh {
            let members = self.workspace_projects(&ws.id)?;
            let network = format!("trestle-{}-mesh", ws.name);
            remove_mesh_if_idle(self.runtime.engine(), &network, &members)
                .await
                .map_err(engine_error)?;
        }
        Ok(())
    }

    /// Every instance's container changes from now on, from Docker's events (any
    /// process's instances, the CLI's included). Ends when the event stream fails
    /// (the daemon went away): call it again to resume.
    pub async fn changes(&self) -> Result<impl Stream<Item = InstanceChange> + Send + 'static> {
        let events = self
            .runtime
            .engine()
            .events(&EventFilter::All)
            .await
            .map_err(engine_error)?;
        Ok(events
            .take_while(|e| futures::future::ready(e.is_ok()))
            .filter_map(|e| {
                let change = e.ok().and_then(|e| {
                    let (workspace, _, _) = parse_instance_project(&e.project)?;
                    Some(InstanceChange {
                        workspace: workspace.to_owned(),
                        action: action_word(&e.action),
                        service: e.service,
                        project: e.project,
                    })
                });
                futures::future::ready(change)
            }))
    }

    /// Match what Docker runs against the store, as at launch: a running instance is
    /// recorded as wanted running (adopted), one wanted running whose project is gone
    /// as stopped (stale). Instance projects the store doesn't know are reported, not
    /// removed.
    pub async fn reconcile(&self) -> Result<ReconcileReport> {
        let found = self
            .runtime
            .reconcile(ReconcileOptions {
                remove_orphans: false,
            })
            .await?;
        let label = |id: &InstanceId| -> Result<InstanceLabel> {
            let instance = self.store.instance(id)?;
            let env = self.store.environment(&instance.environment)?;
            let ws = self.store.workspace_by_id(&env.workspace)?;
            Ok(InstanceLabel {
                id: id.0.clone(),
                label: format!("{}/{} #{}", ws.name, env.name, instance.revision),
            })
        };
        Ok(ReconcileReport {
            adopted: found.adopted.iter().map(label).collect::<Result<_>>()?,
            stale: found.stale.iter().map(label).collect::<Result<_>>()?,
            orphans: found.orphans,
        })
    }

    /// The id of the environment's workspace.
    pub fn workspace_of(&self, env_id: &str) -> Result<String> {
        Ok(self.store.environment(&Self::env_id(env_id))?.workspace.0)
    }

    /// Whether the instance's project runs in Docker.
    pub async fn is_running(&self, instance_id: &str) -> Result<bool> {
        let instance = self.store.instance(&InstanceId(instance_id.to_owned()))?;
        let env = self.store.environment(&instance.environment)?;
        let ws = self.store.workspace_by_id(&env.workspace)?;
        let project = instance_project(&ws.name, &env.name, instance.revision);
        Ok(self.running_projects().await?.contains(&project))
    }

    /// The workspace's environments, each with its instances and their containers.
    pub async fn workspace(&self, workspace_id: &str) -> Result<WorkspaceRun> {
        let ws = WorkspaceId(workspace_id.to_owned());
        let mut environments = Vec::new();
        let mut engine_problem = None;
        let mut envs = self.store.environments(&ws)?;
        envs.sort_by(|a, b| a.name.cmp(&b.name));
        for env in envs {
            let mut instances = Vec::new();
            for instance in self.store.instances_of(&env.id)? {
                let ask = engine_problem.is_none();
                match self.instance_status(&instance, ask).await {
                    Ok(status) => instances.push(status),
                    Err(RunError::Unavailable { message }) => {
                        engine_problem = Some(message);
                        instances.push(self.instance_status(&instance, false).await?);
                    }
                    Err(e) => return Err(e),
                }
            }
            instances.sort_by_key(|i| std::cmp::Reverse(i.revision));
            environments.push(EnvironmentRun {
                id: env.id.0,
                name: env.name,
                head: env.head,
                instances,
            });
        }
        Ok(WorkspaceRun {
            environments,
            engine_problem,
        })
    }

    /// `instance` with its containers (when `ask_engine`) and its gateway URL.
    async fn instance_status(
        &self,
        instance: &Instance,
        ask_engine: bool,
    ) -> Result<InstanceStatus> {
        let env = self.store.environment(&instance.environment)?;
        let workspace = self.store.workspace_by_id(&env.workspace)?;
        let project = instance_project(&workspace.name, &env.name, instance.revision);
        let services = if ask_engine {
            self.runtime
                .engine()
                .ps(&project)
                .await
                .map_err(engine_error)?
                .iter()
                .map(service_row)
                .collect()
        } else {
            Vec::new()
        };
        // Only an instance with a slot has its ports: asking for a placement would
        // lease one.
        let gateway_url = match instance.slot {
            Some(_) => self.gateway_url(instance, &env.name)?,
            None => None,
        };
        Ok(InstanceStatus {
            id: instance.id.0.clone(),
            revision: instance.revision,
            desired: match instance.desired {
                Desired::Running => "running",
                Desired::Stopped => "stopped",
            }
            .into(),
            slot: instance.slot,
            project,
            gateway_url,
            services,
        })
    }

    /// The instance's revision, planned at its placement (without secrets, which
    /// neither the gateway URL nor the bindings depend on), as a manifest whose
    /// context carries that placement, and the catalog it plans with.
    fn placed_manifest(
        &self,
        instance: &Instance,
        env_name: &str,
    ) -> Result<(EnvManifest, Catalog)> {
        let revision = self
            .store
            .revision(&instance.environment, instance.revision)?;
        let placement = self.runtime.placement(&instance.id)?;
        let mut manifest: EnvManifest = revision.manifest().clone();
        manifest.context.env_name = env_name.to_owned();
        placement.apply(&mut manifest.context);
        let mut catalog = baseline_catalog();
        for text in revision.modules().values() {
            if let Ok(custom) = Catalog::from_manifests([text.as_str()]) {
                catalog = catalog.merge(custom);
            }
        }
        Ok((manifest, catalog))
    }

    /// The gateway's URL from the host.
    fn gateway_url(&self, instance: &Instance, env_name: &str) -> Result<Option<String>> {
        let (manifest, catalog) = self.placed_manifest(instance, env_name)?;
        Ok(manifest
            .plan(&catalog)
            .ok()
            .and_then(|plan| plan.gateway_url()))
    }

    /// The instance, as the current environment.
    pub fn describe(&self, instance_id: &str) -> Result<CurrentEnvironment> {
        let instance = self.store.instance(&InstanceId(instance_id.to_owned()))?;
        let env = self.store.environment(&instance.environment)?;
        let workspace = self.store.workspace_by_id(&env.workspace)?;
        Ok(CurrentEnvironment::Instance {
            instance_id: instance.id.0,
            workspace_id: workspace.id.0,
            workspace: workspace.name,
            environment_id: env.id.0,
            environment: env.name,
            revision: instance.revision,
        })
    }

    /// What the instance's gateway exposes, by module: each module's root on the
    /// gateway listener (surface) that serves it, `http://localhost:<port>/`, under the
    /// base path its service serves itself at (headwaters' `/lineage`, so its UI's links
    /// resolve behind the gateway). The services' clients send their own paths under
    /// that root, as they do to a Neon Function's URL, where the service is at the root.
    pub fn bindings(&self, instance_id: &str) -> Result<Vec<ServiceRoot>> {
        let instance = self.store.instance(&InstanceId(instance_id.to_owned()))?;
        let env = self.store.environment(&instance.environment)?;
        let (manifest, catalog) = self.placed_manifest(&instance, &env.name)?;
        let Ok(plan) = manifest.plan(&catalog) else {
            return Ok(Vec::new());
        };
        let Some(origin) = plan.gateway_url() else {
            return Ok(Vec::new());
        };
        let dto = plan_result_for_manifest(&manifest, &catalog).map_err(|e| RunError::Other {
            message: e.to_string(),
        })?;
        let origin = reqwest::Url::parse(&origin).map_err(|e| RunError::Other {
            message: format!("gateway URL `{origin}`: {e}"),
        })?;
        let mut roots = BTreeMap::new();
        for surface in dto.graph.nodes.iter().flat_map(|n| &n.surfaces) {
            for exposed in &surface.exposes {
                let base = plan
                    .services
                    .iter()
                    .find(|(id, _)| id.as_str() == exposed.module)
                    .and_then(|(_, specs)| specs.iter().find(|s| !s.base_path.is_empty()))
                    .map_or("", |s| s.base_path.trim_end_matches('/'));
                let mut url = origin.clone();
                url.set_path(&format!("{base}/"));
                url.set_query(None);
                let _ = url.set_port(Some(surface.host_port));
                roots.entry(exposed.module.clone()).or_insert(url);
            }
        }
        Ok(roots
            .into_iter()
            .map(|(module_id, url)| ServiceRoot {
                module_id,
                url: url.to_string(),
            })
            .collect())
    }

    async fn running_projects(&self) -> Result<BTreeSet<String>> {
        match self.runtime.engine().ls().await {
            Ok(projects) => Ok(projects
                .into_iter()
                .filter(|p| p.is_running())
                .map(|p| p.name)
                .collect()),
            // Nothing can be running without a daemon.
            Err(e) if e.is_unavailable() => Ok(BTreeSet::new()),
            Err(e) => Err(engine_error(e)),
        }
    }

    fn workspace_projects(&self, workspace: &WorkspaceId) -> Result<BTreeSet<String>> {
        let ws = self.store.workspace_by_id(workspace)?;
        let mut members = BTreeSet::new();
        for env in self.store.environments(workspace)? {
            for instance in self.store.instances_of(&env.id)? {
                members.insert(instance_project(&ws.name, &env.name, instance.revision));
            }
        }
        Ok(members)
    }
}

#[cfg(test)]
mod tests {
    use olai_stack_runtime::FakeEngine;

    use super::*;
    use crate::state::SharedState;

    struct Fixture {
        _dir: tempfile::TempDir,
        state: SharedState,
        engine: FakeEngine,
        runner: Runner<FakeEngine>,
        dev: String,
        workspace: String,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let state = SharedState::open(Some(dir.path())).unwrap();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let dev = state.workspace(&ws.id).unwrap().environments.remove(0).id;
        let engine = FakeEngine::new();
        let runner = Runner::with_engine(state.store(), engine.clone(), state.home());
        Fixture {
            _dir: dir,
            state,
            engine,
            runner,
            dev,
            workspace: ws.id,
        }
    }

    #[tokio::test]
    async fn start_needs_a_revision() {
        let f = fixture();
        assert_eq!(
            f.runner.start(&f.dev, false).await,
            Err(RunError::NoRevision { env: "dev".into() })
        );
        assert!(f.engine.calls().is_empty());
    }

    #[tokio::test]
    async fn start_runs_the_head_revision_and_reports_its_containers() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        f.engine.script(
            "local_dev_r1",
            [
                ("postgres", Health::Healthy),
                ("unity-catalog", Health::Starting),
            ],
        );

        let started = f.runner.start(&f.dev, false).await.unwrap();
        assert_eq!(started.revision, 1);
        assert_eq!(started.desired, "running");
        assert_eq!(started.slot, Some(0));
        assert_eq!(started.project, "local_dev_r1");
        assert_eq!(
            started.gateway_url.as_deref(),
            Some("http://localhost:9080")
        );
        let health: Vec<(&str, &str)> = started
            .services
            .iter()
            .map(|s| (s.service.as_str(), s.health.as_str()))
            .collect();
        assert_eq!(
            health,
            [("postgres", "healthy"), ("unity-catalog", "starting")]
        );

        // Starting again reuses the instance (and its slot).
        let again = f.runner.start(&f.dev, false).await.unwrap();
        assert_eq!(again.id, started.id);

        let run = f.runner.workspace(&f.workspace).await.unwrap();
        assert_eq!(run.engine_problem, None);
        assert_eq!(run.environments.len(), 1);
        assert_eq!(run.environments[0].head, Some(1));
        assert_eq!(run.environments[0].instances, vec![again]);
    }

    #[tokio::test]
    async fn stop_takes_it_down_and_keeps_the_slot() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        let started = f.runner.start(&f.dev, false).await.unwrap();
        f.runner.stop(&f.dev).await.unwrap();
        let run = f.runner.workspace(&f.workspace).await.unwrap();
        let instance = &run.environments[0].instances[0];
        assert_eq!(instance.id, started.id);
        assert_eq!(instance.desired, "stopped");
        assert_eq!(instance.slot, Some(0));
        assert!(instance.services.is_empty(), "{:?}", instance.services);
    }

    #[tokio::test]
    async fn a_failed_up_stays_wanted_running_until_stopped() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        f.engine.fail_up("local_dev_r1");
        assert!(matches!(
            f.runner.start(&f.dev, false).await,
            Err(RunError::Compose { .. }) | Err(RunError::Other { .. })
        ));
        let run = f.runner.workspace(&f.workspace).await.unwrap();
        assert_eq!(run.environments[0].instances[0].desired, "running");
        f.runner.stop(&f.dev).await.unwrap();
        let run = f.runner.workspace(&f.workspace).await.unwrap();
        assert_eq!(run.environments[0].instances[0].desired, "stopped");
    }

    #[tokio::test]
    async fn without_docker_the_status_says_so() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        f.runner.start(&f.dev, false).await.unwrap();
        f.engine.set_unavailable(true);
        let run = f.runner.workspace(&f.workspace).await.unwrap();
        assert!(run.engine_problem.is_some());
        assert_eq!(run.environments[0].instances[0].desired, "running");
        assert!(matches!(
            f.runner.start(&f.dev, false).await,
            Err(RunError::Unavailable { .. })
        ));
    }

    #[tokio::test]
    async fn a_running_instance_binds_each_service_at_its_own_root_on_the_gateway() {
        let f = fixture();
        let mut selection = f.state.workspace(&f.workspace).unwrap().environments[0]
            .selection
            .clone();
        let modules = selection.0["modules"].as_array_mut().unwrap();
        if !modules.iter().any(|m| m == "headwaters") {
            modules.push("headwaters".into());
        }
        f.state.set_selection(&f.dev, selection).unwrap();
        f.state.commit(&f.dev).unwrap();
        let started = f.runner.start(&f.dev, false).await.unwrap();
        let bindings = f.runner.bindings(&started.id).unwrap();
        let uc = bindings
            .iter()
            .find(|b| b.module_id == "unity-catalog")
            .unwrap_or_else(|| panic!("unity-catalog in {bindings:?}"));
        assert_eq!(
            uc.url, "http://localhost:9080/",
            "the gateway root, no prefix"
        );
        // headwaters serves itself under /lineage behind the gateway: that's its root.
        let headwaters = bindings
            .iter()
            .find(|b| b.module_id == "headwaters")
            .unwrap_or_else(|| panic!("headwaters in {bindings:?}"));
        assert_eq!(headwaters.url, "http://localhost:9080/lineage/");
        // RustFS has its own (S3) listener.
        let rustfs = bindings.iter().find(|b| b.module_id == "rustfs");
        assert!(
            rustfs.is_some_and(|b| b.url.starts_with("http://localhost:") && b.url != uc.url),
            "{bindings:?}"
        );
    }

    #[tokio::test]
    async fn a_new_revision_starts_as_a_new_instance_on_its_own_slot() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        f.runner.start(&f.dev, false).await.unwrap();
        // Change the draft and commit revision 2.
        let mut selection = f.state.workspace(&f.workspace).unwrap().environments[0]
            .selection
            .clone();
        selection.0["modules"].as_array_mut().unwrap().pop();
        f.state.set_selection(&f.dev, selection).unwrap();
        f.state.commit(&f.dev).unwrap();
        let second = f.runner.start(&f.dev, false).await.unwrap();
        assert_eq!((second.revision, second.slot), (2, Some(1)));
        assert_eq!(
            second.gateway_url.as_deref(),
            Some("http://localhost:10080")
        );
    }

    #[tokio::test]
    async fn one_at_a_time_stops_the_workspace_s_other_instances_first() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        let prod = f
            .state
            .create_environment(&f.workspace, "prod", None)
            .unwrap();
        f.state.commit(&prod.id).unwrap();

        let dev = f.runner.start(&f.dev, false).await.unwrap();
        let side = f.runner.start(&prod.id, false).await.unwrap();
        assert_eq!(f.engine.projects().len(), 2, "side by side by default");

        // A new revision of dev, started one at a time: prod and dev #1 stop.
        let mut selection = f.state.workspace(&f.workspace).unwrap().environments[0]
            .selection
            .clone();
        selection.0["modules"]
            .as_array_mut()
            .unwrap()
            .push("jaeger".into());
        f.state.set_selection(&f.dev, selection).unwrap();
        f.state.commit(&f.dev).unwrap();
        let only = f.runner.start(&f.dev, true).await.unwrap();
        assert_eq!(only.revision, 2);
        assert_eq!(
            f.engine.projects().into_iter().collect::<Vec<_>>(),
            ["local_dev_r2"]
        );
        let run = f.runner.workspace(&f.workspace).await.unwrap();
        let desired = |id: &str| {
            run.environments
                .iter()
                .flat_map(|e| &e.instances)
                .find(|i| i.id == id)
                .map(|i| i.desired.clone())
        };
        assert_eq!(desired(&dev.id).as_deref(), Some("stopped"));
        assert_eq!(desired(&side.id).as_deref(), Some("stopped"));
        assert_eq!(desired(&only.id).as_deref(), Some("running"));
    }

    #[tokio::test]
    async fn stop_instance_stops_only_that_one() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        let prod = f
            .state
            .create_environment(&f.workspace, "prod", None)
            .unwrap();
        f.state.commit(&prod.id).unwrap();
        let dev = f.runner.start(&f.dev, false).await.unwrap();
        f.runner.start(&prod.id, false).await.unwrap();
        f.runner.stop_instance(&dev.id).await.unwrap();
        assert_eq!(
            f.engine.projects().into_iter().collect::<Vec<_>>(),
            ["local_prod_r1"]
        );
        assert!(!f.runner.is_running(&dev.id).await.unwrap());
    }

    #[tokio::test]
    async fn changes_follow_every_instance_s_containers() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        f.engine
            .script("local_dev_r1", [("postgres", Health::Healthy)]);
        f.runner.start(&f.dev, false).await.unwrap();

        let mut changes = Box::pin(f.runner.changes().await.unwrap());
        // Not an instance project: ignored.
        f.engine.adopt("someone-else");
        f.engine
            .set_health("someone-else", "app", Health::Unhealthy);
        f.engine
            .set_health("local_dev_r1", "postgres", Health::Unhealthy);
        let change = tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .expect("a change within 5 s")
            .expect("the stream is open");
        assert_eq!(
            change,
            InstanceChange {
                project: "local_dev_r1".into(),
                workspace: "local".into(),
                service: "postgres".into(),
                action: "health:unhealthy".into(),
            }
        );
    }

    #[tokio::test]
    async fn reconcile_adopts_what_runs_and_stops_what_is_gone() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        let prod = f
            .state
            .create_environment(&f.workspace, "prod", None)
            .unwrap();
        f.state.commit(&prod.id).unwrap();
        // dev runs (as the CLI started it before the app launched); prod was wanted
        // running, but its project is gone (the daemon restarted).
        let dev = f.runner.start(&f.dev, false).await.unwrap();
        let gone = f.runner.start(&prod.id, false).await.unwrap();

        let after = FakeEngine::new();
        after.adopt("local_dev_r1");
        after.adopt("local_old_r9");
        let relaunched = Runner::with_engine(f.state.store(), after, f.state.home());
        let report = relaunched.reconcile().await.unwrap();
        assert_eq!(
            report.adopted,
            vec![InstanceLabel {
                id: dev.id.clone(),
                label: "local/dev #1".into()
            }]
        );
        assert_eq!(
            report.stale,
            vec![InstanceLabel {
                id: gone.id.clone(),
                label: "local/prod #1".into()
            }]
        );
        assert_eq!(report.orphans, ["local_old_r9"]);
        assert!(!report.is_quiet());
        assert!(relaunched.is_running(&dev.id).await.unwrap());
        let run = relaunched.workspace(&f.workspace).await.unwrap();
        let prod_run = run.environments.iter().find(|e| e.name == "prod").unwrap();
        assert_eq!(prod_run.instances[0].desired, "stopped");
    }
}
