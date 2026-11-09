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
use olai_stack_runtime::neon::NeonBranchSource;
use olai_stack_runtime::{
    ComposeEngine, DockerCli, EnvProvider, EventAction, EventFilter, Health, InstanceError,
    Instances, KeychainProvider, LogLine, LogOptions, ReconcileOptions, SecretProviders,
    ServiceStatus, State, instance_project, parse_instance_project, remove_mesh_if_idle,
};
use olai_stack_state::{
    Desired, EnvironmentId, Instance, InstanceId, Revision, Store, WorkspaceId,
};
use olai_stack_topology::manifest::EnvManifest;
use olai_stack_topology::projection::plan_result_for_manifest;
use olai_stack_topology::{Catalog, NeonRequirements, Plan, baseline_catalog};
use serde::Serialize;

use crate::kek;
use crate::keys::ServiceKeys;
use crate::neon::{
    BoundBranch, NeonEnvironments, NeonNeeds, NeonTarget, SecretSink, binding_of, bound_branch,
};

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
    /// The key the views address the service's module by ([`ServiceKeys`]: the API it
    /// serves, `unity-catalog`, else its module id), from the revision's plan. A
    /// service's name isn't its module's: Unity Catalog runs as `unitycatalog`. `None`
    /// for a container the plan doesn't list, such as a module's one-shot helper.
    pub module: Option<String>,
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
    /// The Neon branch it runs on (`trestle up --neon-branch`), if any.
    pub branch: Option<BoundBranch>,
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

/// One line a container of an instance logged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// The compose service (`postgres`), what the viewer filters on.
    pub service: String,
    /// The container as Compose names it (`postgres-1`).
    pub container: String,
    /// RFC 3339, as Docker recorded it.
    pub timestamp: Option<String>,
    pub text: String,
}

impl From<LogLine> for LogEntry {
    fn from(line: LogLine) -> Self {
        // Compose prefixes a line with `<service>-<n>`.
        let service = match line.container.rsplit_once('-') {
            Some((service, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
                service.to_owned()
            }
            _ => line.container.clone(),
        };
        LogEntry {
            service,
            container: line.container,
            timestamp: line.timestamp,
            text: line.text,
        }
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

/// The service UIs that open in a window of their own (they keep a cookie session,
/// which an iframe under the app's origin can't), by module, with their names.
pub const WINDOWED_UIS: &[(&str, &str)] = &[
    ("mlflow", "MLflow"),
    ("jaeger", "Jaeger"),
    ("marimo", "marimo"),
    ("omnigent", "Omnigent"),
];

/// A service UI of a running instance, to open in its own window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ServiceUi {
    pub module_id: String,
    pub name: String,
    /// On the instance's own host name, `http://<host label>.localhost:<port>/<base>/`,
    /// so two instances' cookies stay apart (cookies ignore ports).
    pub url: String,
}

/// `name` as one DNS label: lowercase letters, digits and dashes.
fn dns_label(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| match c.to_ascii_lowercase() {
            c @ ('a'..='z' | '0'..='9') => c,
            _ => '-',
        })
        .collect();
    let label = mapped.trim_matches('-');
    if label.is_empty() {
        "trestle".into()
    } else {
        label.chars().take(63).collect()
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
/// running instance (its gateway's services, and the functions of the Neon branch it
/// runs on).
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
}

impl CurrentEnvironment {
    /// The key the UI remounts on: one per instance or branch.
    pub fn key(&self) -> String {
        let CurrentEnvironment::Instance { instance_id, .. } = self;
        format!("instance:{instance_id}")
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
    /// The revision runs on Neon (`needs`) and the instance is bound to no branch.
    #[error("{env} runs on Neon ({needs}), but no branch was chosen")]
    NeedsBranch { env: String, needs: String },
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
        module: None,
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
    /// The workspaces' Neon projects, when the Neon CLI is there: binding instances to
    /// branches, and their functions' URLs.
    neon: Option<Arc<NeonEnvironments>>,
    /// Where Start mints an environment's Unity Catalog key ([`crate::kek`]): the
    /// CLI's keychain. Without one, a revision that needs the key doesn't start.
    secrets: Option<Arc<dyn SecretSink>>,
}

impl Runner<DockerCli> {
    /// On the `docker` on `PATH`, with secrets from the process environment and the
    /// keychain (as the CLI resolves them), and Neon branches through `neon` when the
    /// Neon CLI is installed.
    pub fn docker(store: Arc<Store>, home: &Path, neon: Option<Arc<NeonEnvironments>>) -> Self {
        let providers = SecretProviders::new()
            .with(EnvProvider)
            .with(KeychainProvider::new(KEYCHAIN_SERVICE));
        let runtime = Instances::new(Arc::clone(&store), DockerCli::new(), home)
            .with_providers(providers.clone())
            .with_wait_timeout(START_TIMEOUT);
        let runner = Runner {
            store,
            runtime,
            neon: None,
            secrets: Some(Arc::new(KeychainProvider::new(KEYCHAIN_SERVICE))),
        };
        match neon {
            Some(neon) => runner.with_neon(neon, providers),
            None => runner,
        }
    }
}

impl<E: ComposeEngine> Runner<E> {
    /// On `engine`, with no secret providers beyond the process environment and no
    /// host-port probe (tests).
    pub fn with_engine(store: Arc<Store>, engine: E, home: &Path) -> Self {
        let runtime = Instances::new(Arc::clone(&store), engine, home)
            .without_port_probe()
            .with_wait_timeout(START_TIMEOUT);
        Runner {
            store,
            runtime,
            neon: None,
            secrets: None,
        }
    }

    /// Mint keys into `secrets`, and resolve the instances' secrets with `providers`,
    /// which must read what `secrets` writes (tests).
    pub fn with_secrets(self, secrets: Arc<dyn SecretSink>, providers: SecretProviders) -> Self {
        Runner {
            runtime: self.runtime.with_providers(providers),
            secrets: Some(secrets),
            ..self
        }
    }

    /// Neon branches through `neon`: the runtime resolves a bound instance's branch
    /// (its `neon` source) with `providers` for the storage credential's secret.
    pub fn with_neon(self, neon: Arc<NeonEnvironments>, providers: SecretProviders) -> Self {
        Runner {
            runtime: self
                .runtime
                .with_source(NeonBranchSource::new(neon.control(), providers)),
            neon: Some(neon),
            ..self
        }
    }

    fn neon(&self) -> Result<&Arc<NeonEnvironments>> {
        self.neon.as_ref().ok_or_else(|| RunError::Neon {
            message: "the Neon CLI isn't available (see Doctor)".into(),
        })
    }

    /// What revision `revision` of `env` needs of a Neon branch, from its plan (as
    /// `trestle up` asks), and the keys the views address its services by.
    fn requirements(
        &self,
        env: &EnvironmentId,
        revision: u32,
    ) -> Result<(NeonRequirements, ServiceKeys)> {
        let (plan, catalog) = self.revision_plan(env, revision)?;
        let req = plan.neon_requirements();
        let keys = ServiceKeys::new(&catalog, &req);
        Ok((req, keys))
    }

    /// Revision `revision` of `env`, planned (without a placement), and the catalog
    /// it plans with.
    fn revision_plan(&self, env: &EnvironmentId, revision: u32) -> Result<(Plan, Catalog)> {
        let revision = self.store.revision(env, revision)?;
        let catalog = revision_catalog(&revision);
        let plan = revision
            .manifest()
            .plan(&catalog)
            .map_err(|e| RunError::Other {
                message: e.to_string(),
            })?;
        Ok((plan, catalog))
    }

    /// Mint `env`'s Unity Catalog key if revision `revision` runs a server that
    /// needs it locally and the environment has none ([`crate::kek`]).
    fn ensure_kek(&self, env: &EnvironmentId, revision: u32) -> Result<()> {
        let (plan, catalog) = self.revision_plan(env, revision)?;
        if !kek::needs_kek(&plan, &catalog) {
            return Ok(());
        }
        let secrets = self.secrets.as_deref().ok_or_else(|| RunError::Other {
            message: format!(
                "Unity Catalog needs {} and there is no keychain to keep it in",
                kek::UC_KEK_VAR
            ),
        })?;
        kek::ensure_kek(&self.store, env, secrets)?;
        Ok(())
    }

    /// The keys the views address the services of revision `revision` of `env` by.
    /// Without a plan, functions keep their slugs.
    fn service_keys(&self, env: &EnvironmentId, revision: u32) -> Result<ServiceKeys> {
        match self.requirements(env, revision) {
            Ok((_, keys)) => Ok(keys),
            Err(RunError::Other { .. }) => {
                let revision = self.store.revision(env, revision)?;
                Ok(ServiceKeys::new(
                    &revision_catalog(&revision),
                    &NeonRequirements::default(),
                ))
            }
            Err(e) => Err(e),
        }
    }

    /// What the environment's head revision needs of a Neon branch: what Start asks
    /// for. Nothing without a revision.
    pub fn neon_needs(&self, env_id: &str) -> Result<NeonNeeds> {
        let env = self.store.environment(&Self::env_id(env_id))?;
        match env.head {
            Some(head) => {
                let (req, keys) = self.requirements(&env.id, head)?;
                Ok(NeonNeeds::of(&req, &keys))
            }
            None => Ok(NeonNeeds::default()),
        }
    }

    /// The URLs of the functions the instance's branch runs for it, by the key the
    /// views address them by (none when it isn't bound to one).
    pub async fn function_roots(&self, instance_id: &str) -> Result<Vec<ServiceRoot>> {
        let instance = self.store.instance(&InstanceId(instance_id.to_owned()))?;
        let Some(binding) = binding_of(&instance) else {
            return Ok(Vec::new());
        };
        let keys = self.service_keys(&instance.environment, instance.revision)?;
        self.neon()?.function_roots(&binding, &keys).await
    }

    /// Before branch `branch_id` goes: unbind the workspace's stopped instances on it.
    /// Refused while one of them runs.
    pub async fn release_branch(&self, workspace_id: &str, branch_id: &str) -> Result<()> {
        let ws = WorkspaceId(workspace_id.to_owned());
        let mut bound = Vec::new();
        for env in self.store.environments(&ws)? {
            for instance in self.store.instances_of(&env.id)? {
                if binding_of(&instance).is_some_and(|b| b.branch == branch_id) {
                    if instance.desired == Desired::Running {
                        return Err(RunError::Neon {
                            message: format!(
                                "{} #{} runs on the branch: stop it first",
                                env.name, instance.revision
                            ),
                        });
                    }
                    bound.push(instance);
                }
            }
        }
        for instance in bound {
            self.neon()?.unbind_instance(&instance).await?;
        }
        Ok(())
    }

    /// Undo the instance's branch binding (its storage credential revoked).
    pub async fn unbind(&self, instance_id: &str) -> Result<()> {
        let instance = self.store.instance(&InstanceId(instance_id.to_owned()))?;
        if binding_of(&instance).is_none() {
            return Ok(());
        }
        self.neon()?.unbind_instance(&instance).await
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
    ///
    /// With `target`, the instance is bound to that Neon branch first, as `trestle up
    /// --neon-branch` binds it; it stays bound for later starts. A revision that runs
    /// on Neon (function-hosted modules, the branch's Postgres or storage) refuses to
    /// start unbound ([`RunError::NeedsBranch`]).
    pub async fn start(
        &self,
        env_id: &str,
        one_at_a_time: bool,
        target: Option<&NeonTarget>,
    ) -> Result<InstanceStatus> {
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
        let (req, keys) = self.requirements(&env.id, head)?;
        match target {
            Some(target) => {
                let ws = self.store.workspace_by_id(&env.workspace)?;
                self.neon()?
                    .bind_instance(&ws, &env, &instance, head, target, &req)
                    .await?;
            }
            None if !req.is_empty() && binding_of(&instance).is_none() => {
                return Err(RunError::NeedsBranch {
                    env: env.name.clone(),
                    needs: NeonNeeds::of(&req, &keys).describe(),
                });
            }
            None => {}
        }
        if one_at_a_time {
            for other in self.live_instances(&env.workspace).await? {
                if other != instance.id {
                    self.stop_instance(&other.0).await?;
                }
            }
        }
        self.ensure_kek(&env.id, head)?;
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

    /// The instance's logs (`docker compose logs`): the last `tail` lines of each
    /// service (every line without it), then, with `follow`, what they log from now
    /// on. Ends when the containers do (or the stream fails), or when dropped.
    pub async fn logs(
        &self,
        instance_id: &str,
        tail: Option<u32>,
        follow: bool,
    ) -> Result<impl Stream<Item = std::result::Result<LogEntry, String>> + Send + 'static> {
        // Asking for a placement leases a slot: an instance without one never ran.
        let instance = self.store.instance(&InstanceId(instance_id.to_owned()))?;
        if instance.slot.is_none() {
            return Err(RunError::Other {
                message: "the instance hasn't been started, so it has no logs".into(),
            });
        }
        let project = self
            .runtime
            .placement(&InstanceId(instance_id.to_owned()))?
            .project_name;
        let lines = self
            .runtime
            .engine()
            .logs(
                &project,
                &LogOptions {
                    follow,
                    tail,
                    services: Vec::new(),
                },
            )
            .await
            .map_err(engine_error)?;
        Ok(lines.map(|line| line.map(LogEntry::from).map_err(|e| e.to_string())))
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
        let mut services: Vec<ServiceRow> = if ask_engine {
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
        let (gateway_url, modules) = match instance.slot {
            Some(_) => self.plan_facts(instance, &env.name)?,
            None => (None, BTreeMap::new()),
        };
        for row in &mut services {
            row.module = modules.get(&row.service).cloned();
        }
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
            branch: bound_branch(
                instance,
                &self.service_keys(&instance.environment, instance.revision)?,
            ),
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
        Ok((manifest, revision_catalog(&revision)))
    }

    /// From the instance's plan at its placement: the gateway's URL from the host, and
    /// the key of the module each compose service belongs to ([`ServiceKeys`]), by
    /// service name.
    fn plan_facts(
        &self,
        instance: &Instance,
        env_name: &str,
    ) -> Result<(Option<String>, BTreeMap<String, String>)> {
        let (manifest, catalog) = self.placed_manifest(instance, env_name)?;
        let Ok(plan) = manifest.plan(&catalog) else {
            return Ok((None, BTreeMap::new()));
        };
        let keys = ServiceKeys::new(&catalog, &plan.neon_requirements());
        let modules = plan
            .services
            .iter()
            .flat_map(|(id, specs)| {
                let key = keys.module(id.as_str());
                specs
                    .iter()
                    .map(move |spec| (spec.name.to_string(), key.clone()))
            })
            .collect();
        Ok((plan.gateway_url(), modules))
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

    /// What the instance's gateway exposes, by the key the views address each module by
    /// ([`ServiceKeys`]): each module's root on the
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
        let keys = ServiceKeys::new(&catalog, &plan.neon_requirements());
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
                roots.entry(keys.module(&exposed.module)).or_insert(url);
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

    /// The instance's service UIs that open in their own window ([`WINDOWED_UIS`]), at
    /// their gateway roots on the instance's own host name.
    pub fn service_uis(&self, instance_id: &str) -> Result<Vec<ServiceUi>> {
        let label = dns_label(
            &self
                .runtime
                .placement(&InstanceId(instance_id.to_owned()))?
                .host_label,
        );
        let roots = self.bindings(instance_id)?;
        let mut uis = Vec::new();
        for (module, name) in WINDOWED_UIS {
            let Some(root) = roots.iter().find(|r| r.module_id == *module) else {
                continue;
            };
            let mut url = reqwest::Url::parse(&root.url).map_err(|e| RunError::Other {
                message: format!("`{}`: {e}", root.url),
            })?;
            url.set_host(Some(&format!("{label}.localhost")))
                .map_err(|e| RunError::Other {
                    message: e.to_string(),
                })?;
            uis.push(ServiceUi {
                module_id: (*module).to_owned(),
                name: (*name).to_owned(),
                url: url.to_string(),
            });
        }
        Ok(uis)
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

/// The catalog `revision` plans with: the baseline, plus the revision's own modules.
fn revision_catalog(revision: &Revision) -> Catalog {
    let mut catalog = baseline_catalog();
    for text in revision.modules().values() {
        if let Ok(custom) = Catalog::from_manifests([text.as_str()]) {
            catalog = catalog.merge(custom);
        }
    }
    catalog
}

#[cfg(test)]
mod tests {
    use olai_stack_runtime::{FakeCall, FakeEngine};

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

    /// A keychain in memory that the runtime also resolves `keychain` secrets from.
    #[derive(Clone, Default)]
    struct Keychain(Arc<crate::neon::tests::MemorySecrets>);

    impl olai_stack_runtime::SecretProvider for Keychain {
        fn name(&self) -> &str {
            "keychain"
        }
        fn get(
            &self,
            key: &str,
        ) -> std::result::Result<olai_stack_runtime::SecretValue, olai_stack_runtime::SecretError>
        {
            self.0
                .0
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .map(olai_stack_runtime::SecretValue::new)
                .ok_or_else(|| olai_stack_runtime::SecretError::Missing {
                    provider: "keychain".into(),
                    key: key.into(),
                })
        }
    }

    /// The fixture with a keychain in memory.
    fn keychain_fixture() -> (Fixture, Keychain) {
        let f = fixture();
        let keychain = Keychain::default();
        let runner = Runner::with_engine(f.state.store(), f.engine.clone(), f.state.home())
            .with_secrets(
                keychain.0.clone(),
                SecretProviders::new().with(keychain.clone()),
            );
        (Fixture { runner, ..f }, keychain)
    }

    /// Replace the environment's Unity Catalog with `implementation`, in a container.
    fn run_unity_catalog(f: &Fixture, implementation: &str) {
        let mut selection = f.state.workspace(&f.workspace).unwrap().environments[0]
            .selection
            .clone();
        let modules = selection.0["modules"].as_array_mut().unwrap();
        modules.retain(|m| !m.as_str().is_some_and(|m| m.starts_with("unity-catalog")));
        modules.push(implementation.into());
        f.state.set_selection(&f.dev, selection).unwrap();
    }

    /// The environment passed to the last `up`.
    fn last_up(engine: &FakeEngine) -> (std::path::PathBuf, BTreeMap<String, String>) {
        engine
            .calls()
            .into_iter()
            .rev()
            .find_map(|c| match c {
                FakeCall::Up { dir, env, .. } => Some((dir, env)),
                _ => None,
            })
            .expect("an up")
    }

    /// The fixture with Neon: project `p`, whose default branch `main` runs the
    /// `unitycatalog` and `headwaters` functions.
    fn neon_fixture() -> (Fixture, Arc<olai_stack_runtime::neon::FakeNeon>) {
        neon_fixture_with(Some("p"))
    }

    /// [`neon_fixture`], the workspaces' project defaulting to `env_project`.
    fn neon_fixture_with(
        env_project: Option<&str>,
    ) -> (Fixture, Arc<olai_stack_runtime::neon::FakeNeon>) {
        let f = fixture();
        let fake = Arc::new(olai_stack_runtime::neon::FakeNeon::new("p"));
        fake.deploy_function("main", "unitycatalog");
        fake.deploy_function("main", "headwaters");
        let neon = Arc::new(NeonEnvironments::new(
            f.state.store(),
            fake.clone(),
            Arc::new(crate::neon::tests::MemorySecrets::default()),
            env_project.map(Into::into),
        ));
        let runner = Runner::with_engine(f.state.store(), f.engine.clone(), f.state.home())
            .with_neon(neon, SecretProviders::new());
        (Fixture { runner, ..f }, fake)
    }

    /// `modules`, each of `functions` hosted as a Neon Function.
    fn on_neon(modules: &[&str], functions: &[&str]) -> crate::Json {
        let hosted: serde_json::Map<String, serde_json::Value> = functions
            .iter()
            .map(|m| {
                (
                    (*m).to_owned(),
                    serde_json::json!({ "vendor": "neon-function", "connection": { "kind": "function" } }),
                )
            })
            .collect();
        serde_json::json!({ "modules": modules, "hosted": hosted }).into()
    }

    #[tokio::test]
    async fn a_selection_from_before_the_unity_catalog_split_still_plans() {
        // `unity-catalog` as a Neon Function, as stored before trestle split it into
        // `unity-catalog-oss` and `-rs`: it reads as `unity-catalog-rs`, still hosted.
        let (f, _) = neon_fixture();
        f.state
            .set_selection(&f.dev, on_neon(&["unity-catalog"], &["unity-catalog"]))
            .unwrap();
        f.state.commit(&f.dev).unwrap();
        let needs = f.runner.neon_needs(&f.dev).unwrap();
        assert_eq!(needs.functions, ["unity-catalog"]);
        let started = f
            .runner
            .start(
                &f.dev,
                false,
                Some(&NeonTarget {
                    project: None,
                    branch: crate::neon::BranchChoice::Existing {
                        branch: "main".into(),
                    },
                }),
            )
            .await
            .unwrap();
        let roots = f.runner.function_roots(&started.id).await.unwrap();
        assert_eq!(roots.len(), 1, "{roots:?}");
        assert_eq!(roots[0].module_id, "unity-catalog");
    }

    #[tokio::test]
    async fn either_unity_catalog_is_bound_as_unity_catalog() {
        for implementation in ["unity-catalog-oss", "unity-catalog-rs"] {
            let (f, _keychain) = keychain_fixture();
            run_unity_catalog(&f, implementation);
            f.state.commit(&f.dev).unwrap();
            f.engine
                .script("local_dev_r1", [("unitycatalog", Health::Healthy)]);
            let started = f.runner.start(&f.dev, false, None).await.unwrap();
            assert_eq!(
                started.services[0].module.as_deref(),
                Some("unity-catalog"),
                "{implementation}"
            );
            let bindings = f.runner.bindings(&started.id).unwrap();
            assert!(
                bindings.iter().any(|b| b.module_id == "unity-catalog"),
                "{implementation}: {bindings:?}"
            );
            assert!(
                bindings.iter().all(|b| b.module_id != implementation),
                "{implementation}: {bindings:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_local_unity_catalog_rs_gets_one_key_per_environment_from_the_keychain() {
        let (f, keychain) = keychain_fixture();
        run_unity_catalog(&f, "unity-catalog-rs");
        f.state.commit(&f.dev).unwrap();
        f.runner.start(&f.dev, false, None).await.unwrap();
        let (dir, env) = last_up(&f.engine);
        let kek = env
            .get(kek::UC_KEK_VAR)
            .expect("UC_SERVER_KEK in the launch env");
        assert_eq!(kek.len(), 44, "32 bytes, base64");
        let stored = keychain.0.0.lock().unwrap().clone();
        assert_eq!(stored.len(), 1, "{:?}", stored.keys());
        assert_eq!(stored.values().next(), Some(kek));
        // The store holds a reference, the instance's files nothing of it.
        let vars = f
            .state
            .store()
            .environment(&Runner::<FakeEngine>::env_id(&f.dev))
            .unwrap()
            .vars;
        assert!(matches!(
            vars.get(kek::UC_KEK_VAR),
            Some(olai_stack_state::VarValue::Secret(r)) if r.provider == "keychain"
        ));
        let mut files = vec![f.state.home().join(olai_stack_state::DB_FILE)];
        let mut dirs = vec![dir];
        while let Some(d) = dirs.pop() {
            for entry in std::fs::read_dir(d).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path)
                } else {
                    files.push(path)
                }
            }
        }
        for file in files {
            let bytes = std::fs::read(&file).unwrap();
            assert!(
                !bytes.windows(kek.len()).any(|w| w == kek.as_bytes()),
                "the key is in {}",
                file.display()
            );
        }
        // A restart keeps the key.
        f.runner.stop(&f.dev).await.unwrap();
        f.runner.start(&f.dev, false, None).await.unwrap();
        assert_eq!(last_up(&f.engine).1.get(kek::UC_KEK_VAR), Some(kek));
        assert_eq!(keychain.0.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn without_a_keychain_a_local_unity_catalog_rs_does_not_start() {
        let f = fixture();
        run_unity_catalog(&f, "unity-catalog-rs");
        f.state.commit(&f.dev).unwrap();
        let err = f.runner.start(&f.dev, false, None).await.unwrap_err();
        assert!(err.to_string().contains(kek::UC_KEK_VAR), "{err}");
        assert!(last_up_none(&f.engine));
    }

    // A function-hosted unity-catalog-rs needs none either: the Neon tests start one
    // on a runner without a keychain.
    #[tokio::test]
    async fn the_oss_server_needs_no_key() {
        let (f, keychain) = keychain_fixture();
        run_unity_catalog(&f, "unity-catalog-oss");
        f.state.commit(&f.dev).unwrap();
        f.runner.start(&f.dev, false, None).await.unwrap();
        assert!(!last_up(&f.engine).1.contains_key(kek::UC_KEK_VAR));
        assert!(keychain.0.0.lock().unwrap().is_empty());
    }

    fn last_up_none(engine: &FakeEngine) -> bool {
        !engine
            .calls()
            .iter()
            .any(|c| matches!(c, FakeCall::Up { .. }))
    }

    #[tokio::test]
    async fn a_revision_on_neon_refuses_to_start_unbound() {
        let (f, _) = neon_fixture();
        f.state
            .set_selection(
                &f.dev,
                on_neon(&["unity-catalog-rs"], &["unity-catalog-rs"]),
            )
            .unwrap();
        f.state.commit(&f.dev).unwrap();
        let needs = f.runner.neon_needs(&f.dev).unwrap();
        assert!(needs.needed);
        assert_eq!(needs.functions, ["unity-catalog"]);
        assert!(matches!(
            f.runner.start(&f.dev, false, None).await,
            Err(RunError::NeedsBranch { env, needs }) if env == "dev" && needs.contains("unity-catalog")
        ));
        assert!(f.engine.calls().is_empty());
    }

    #[tokio::test]
    async fn a_local_revision_needs_no_branch() {
        let (f, _) = neon_fixture();
        f.state.commit(&f.dev).unwrap();
        assert!(!f.runner.neon_needs(&f.dev).unwrap().needed);
        let started = f.runner.start(&f.dev, false, None).await.unwrap();
        assert_eq!(started.branch, None);
        assert!(
            f.runner
                .function_roots(&started.id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn start_on_a_branch_binds_it_and_its_functions() {
        let (f, _) = neon_fixture();
        f.state
            .set_selection(
                &f.dev,
                on_neon(
                    &["unity-catalog-rs", "headwaters"],
                    &["unity-catalog-rs", "headwaters"],
                ),
            )
            .unwrap();
        f.state.commit(&f.dev).unwrap();
        let choice = NeonTarget {
            project: None,
            branch: crate::neon::BranchChoice::Existing {
                branch: "main".into(),
            },
        };
        let started = f.runner.start(&f.dev, false, Some(&choice)).await.unwrap();
        let branch = started.branch.clone().unwrap();
        assert_eq!(branch.name, "main");
        assert_eq!(branch.functions, ["headwaters", "unity-catalog"]);
        // Nothing of the functions runs locally.
        let Some(FakeCall::Up { dir, .. }) = f.engine.calls().into_iter().next() else {
            panic!("no up");
        };
        let compose = std::fs::read_to_string(dir.join("compose.yaml")).unwrap();
        assert!(!compose.contains("unitycatalog:"), "{compose}");
        // The views reach the functions at their URLs.
        let roots = f.runner.function_roots(&started.id).await.unwrap();
        assert_eq!(
            roots,
            [
                ServiceRoot {
                    module_id: "headwaters".into(),
                    url: "https://br-main-headwaters.compute.fake.neon.tech/".into()
                },
                ServiceRoot {
                    module_id: "unity-catalog".into(),
                    url: "https://br-main-unitycatalog.compute.fake.neon.tech/".into()
                },
            ]
        );
        // The gateway fronts them too: each has its root there, headwaters under the base
        // path its container serves at (the gateway cuts it on the way to the function).
        let gateway = f.runner.bindings(&started.id).unwrap();
        let at = |key: &str| {
            gateway
                .iter()
                .find(|r| r.module_id == key)
                .map(|r| r.url.clone())
                .unwrap_or_else(|| panic!("{key} not on the gateway: {gateway:?}"))
        };
        assert!(
            at("unity-catalog").starts_with("http://localhost:"),
            "{gateway:?}"
        );
        assert!(at("headwaters").ends_with("/lineage/"), "{gateway:?}");
        // The envoy config routes to the functions over TLS.
        let envoy = std::fs::read_to_string(dir.join("modules/envoy/envoy.yaml")).unwrap();
        assert!(
            envoy.contains("sni: br-main-unitycatalog.compute.fake.neon.tech"),
            "{envoy}"
        );
        // It stays bound: the next start needs no branch.
        f.runner.stop(&f.dev).await.unwrap();
        let again = f.runner.start(&f.dev, false, None).await.unwrap();
        assert_eq!(again.id, started.id);
        assert_eq!(again.branch.map(|b| b.name).as_deref(), Some("main"));
        // Unbinding a stopped instance drops it.
        f.runner.stop(&f.dev).await.unwrap();
        f.runner.unbind(&started.id).await.unwrap();
        assert!(matches!(
            f.runner.start(&f.dev, false, None).await,
            Err(RunError::NeedsBranch { .. })
        ));
    }

    #[tokio::test]
    async fn start_on_a_new_branch_makes_one_trestle_owns() {
        let (f, fake) = neon_fixture();
        f.state
            .set_selection(
                &f.dev,
                on_neon(&["unity-catalog-rs"], &["unity-catalog-rs"]),
            )
            .unwrap();
        f.state.commit(&f.dev).unwrap();
        // The new branch is a child of the parent (the project's default, `main`), so it
        // runs `main`'s functions, each under its own URL.
        let choice = NeonTarget {
            project: None,
            branch: crate::neon::BranchChoice::New {
                ttl_hours: Some(12),
            },
        };
        let started = f.runner.start(&f.dev, false, Some(&choice)).await.unwrap();
        assert_eq!(started.branch.unwrap().name, "local-dev-r1");
        let branches = olai_stack_runtime::neon::NeonControl::branches(fake.as_ref(), "p")
            .await
            .unwrap();
        let made = branches
            .iter()
            .find(|b| b.name == "local-dev-r1")
            .expect("a branch for the instance");
        assert!(made.is_owned());
        assert!(made.expires_at.is_some());
        let roots = f.runner.function_roots(&started.id).await.unwrap();
        assert_eq!(roots.len(), 1);
        assert!(roots[0].url.contains(&made.id), "{roots:?}");
    }

    #[tokio::test]
    async fn a_branch_is_released_only_once_nothing_runs_on_it() {
        let (f, _) = neon_fixture();
        f.state
            .set_selection(
                &f.dev,
                on_neon(&["unity-catalog-rs"], &["unity-catalog-rs"]),
            )
            .unwrap();
        f.state.commit(&f.dev).unwrap();
        let choice = NeonTarget {
            project: None,
            branch: crate::neon::BranchChoice::Existing {
                branch: "main".into(),
            },
        };
        let started = f.runner.start(&f.dev, false, Some(&choice)).await.unwrap();
        let branch = started.branch.unwrap().id;
        assert!(matches!(
            f.runner.release_branch(&f.workspace, &branch).await,
            Err(RunError::Neon { message }) if message.contains("dev #1 runs on the branch")
        ));
        f.runner.stop(&f.dev).await.unwrap();
        f.runner
            .release_branch(&f.workspace, &branch)
            .await
            .unwrap();
        let run = f.runner.workspace(&f.workspace).await.unwrap();
        assert_eq!(run.environments[0].instances[0].branch, None);
    }

    #[tokio::test]
    async fn an_environment_s_own_project_wins_over_the_workspace_s() {
        // The workspaces default to `DEFAULT_PROJECT`, which FakeNeon doesn't know.
        let (f, _) = neon_fixture_with(None);
        f.state
            .set_selection(
                &f.dev,
                on_neon(&["unity-catalog-rs"], &["unity-catalog-rs"]),
            )
            .unwrap();
        f.state.commit(&f.dev).unwrap();
        let main = |project: Option<&str>| NeonTarget {
            project: project.map(Into::into),
            branch: crate::neon::BranchChoice::Existing {
                branch: "main".into(),
            },
        };
        assert!(
            f.runner
                .start(&f.dev, false, Some(&main(None)))
                .await
                .is_err()
        );
        let started = f
            .runner
            .start(&f.dev, false, Some(&main(Some("p"))))
            .await
            .unwrap();
        assert_eq!(started.branch.unwrap().name, "main");
        let roots = f.runner.function_roots(&started.id).await.unwrap();
        assert_eq!(roots[0].module_id, "unity-catalog");
    }

    #[tokio::test]
    async fn a_branch_without_the_functions_is_refused() {
        let (f, _) = neon_fixture();
        f.state
            .set_selection(
                &f.dev,
                on_neon(&["unity-catalog-rs"], &["unity-catalog-rs"]),
            )
            .unwrap();
        f.state.commit(&f.dev).unwrap();
        let choice = NeonTarget {
            project: None,
            branch: crate::neon::BranchChoice::Existing {
                branch: "nope".into(),
            },
        };
        assert!(matches!(
            f.runner.start(&f.dev, false, Some(&choice)).await,
            Err(RunError::Neon { .. })
        ));
        assert!(f.engine.calls().is_empty());
    }

    #[tokio::test]
    async fn start_needs_a_revision() {
        let f = fixture();
        assert_eq!(
            f.runner.start(&f.dev, false, None).await,
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

        let started = f.runner.start(&f.dev, false, None).await.unwrap();
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
        let again = f.runner.start(&f.dev, false, None).await.unwrap();
        assert_eq!(again.id, started.id);

        let run = f.runner.workspace(&f.workspace).await.unwrap();
        assert_eq!(run.engine_problem, None);
        assert_eq!(run.environments.len(), 1);
        assert_eq!(run.environments[0].head, Some(1));
        assert_eq!(run.environments[0].instances, vec![again]);
    }

    #[tokio::test]
    async fn each_container_names_the_module_it_belongs_to() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        // Compose names a service after its spec, not its module: Unity Catalog's is
        // `unitycatalog`.
        f.engine.script(
            "local_dev_r1",
            [
                ("unitycatalog", Health::Healthy),
                ("not-in-the-plan", Health::Healthy),
            ],
        );
        let started = f.runner.start(&f.dev, false, None).await.unwrap();
        let modules: Vec<(&str, Option<&str>)> = started
            .services
            .iter()
            .map(|s| (s.service.as_str(), s.module.as_deref()))
            .collect();
        assert_eq!(
            modules,
            [
                ("unitycatalog", Some("unity-catalog")),
                ("not-in-the-plan", None)
            ]
        );
    }

    #[tokio::test]
    async fn stop_takes_it_down_and_keeps_the_slot() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        let started = f.runner.start(&f.dev, false, None).await.unwrap();
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
            f.runner.start(&f.dev, false, None).await,
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
        f.runner.start(&f.dev, false, None).await.unwrap();
        f.engine.set_unavailable(true);
        let run = f.runner.workspace(&f.workspace).await.unwrap();
        assert!(run.engine_problem.is_some());
        assert_eq!(run.environments[0].instances[0].desired, "running");
        assert!(matches!(
            f.runner.start(&f.dev, false, None).await,
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
        let started = f.runner.start(&f.dev, false, None).await.unwrap();
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
        f.runner.start(&f.dev, false, None).await.unwrap();
        // Change the draft and commit revision 2.
        let mut selection = f.state.workspace(&f.workspace).unwrap().environments[0]
            .selection
            .clone();
        selection.0["modules"].as_array_mut().unwrap().pop();
        f.state.set_selection(&f.dev, selection).unwrap();
        f.state.commit(&f.dev).unwrap();
        let second = f.runner.start(&f.dev, false, None).await.unwrap();
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

        let dev = f.runner.start(&f.dev, false, None).await.unwrap();
        let side = f.runner.start(&prod.id, false, None).await.unwrap();
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
        let only = f.runner.start(&f.dev, true, None).await.unwrap();
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
        let dev = f.runner.start(&f.dev, false, None).await.unwrap();
        f.runner.start(&prod.id, false, None).await.unwrap();
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
        f.runner.start(&f.dev, false, None).await.unwrap();

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
        let dev = f.runner.start(&f.dev, false, None).await.unwrap();
        let gone = f.runner.start(&prod.id, false, None).await.unwrap();

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

    #[test]
    fn a_log_line_names_its_service() {
        let entry = LogEntry::from(olai_stack_runtime::parse_log_line(
            "unity-catalog-1  | 2026-11-02T19:55:16.807260250Z ready",
        ));
        assert_eq!(entry.service, "unity-catalog");
        assert_eq!(entry.container, "unity-catalog-1");
        assert_eq!(entry.text, "ready");
        assert!(entry.timestamp.is_some());
        // Not compose's `<service>-<n>`: the whole name.
        assert_eq!(
            LogEntry::from(olai_stack_runtime::parse_log_line("gateway | up")).service,
            "gateway"
        );
    }

    #[tokio::test]
    async fn logs_come_from_the_instance_s_project() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        f.engine
            .script("local_dev_r1", [("postgres", Health::Healthy)]);
        let started = f.runner.start(&f.dev, false, None).await.unwrap();
        let lines: Vec<LogEntry> = f
            .runner
            .logs(&started.id, Some(100), false)
            .await
            .unwrap()
            .map(|l| l.unwrap())
            .collect()
            .await;
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].service, "postgres");
        assert!(
            f.engine
                .calls()
                .iter()
                .any(|c| format!("{c:?}").contains("local_dev_r1")),
            "{:?}",
            f.engine.calls()
        );
    }

    #[tokio::test]
    async fn service_uis_open_on_the_instance_s_own_host_name() {
        let f = fixture();
        let mut selection = f.state.workspace(&f.workspace).unwrap().environments[0]
            .selection
            .clone();
        let modules = selection.0["modules"].as_array_mut().unwrap();
        for m in ["mlflow", "jaeger"] {
            if !modules.iter().any(|x| x == m) {
                modules.push(m.into());
            }
        }
        f.state.set_selection(&f.dev, selection).unwrap();
        f.state.commit(&f.dev).unwrap();
        let started = f.runner.start(&f.dev, false, None).await.unwrap();
        let uis = f.runner.service_uis(&started.id).unwrap();
        let mlflow = uis
            .iter()
            .find(|u| u.module_id == "mlflow")
            .unwrap_or_else(|| panic!("mlflow in {uis:?}"));
        assert_eq!(mlflow.name, "MLflow");
        assert_eq!(mlflow.url, "http://local-dev-r1.localhost:9080/mlflow/");
        let jaeger = uis.iter().find(|u| u.module_id == "jaeger");
        assert!(
            jaeger.is_some_and(|j| j.url.starts_with("http://local-dev-r1.localhost:")),
            "{uis:?}"
        );
        assert!(
            uis.iter().all(|u| u.module_id != "unity-catalog"),
            "the data UIs run in the shell"
        );
    }

    #[test]
    fn host_labels_are_dns_labels() {
        assert_eq!(dns_label("local-dev-r1"), "local-dev-r1");
        assert_eq!(dns_label("My_WS-dev-r2"), "my-ws-dev-r2");
        assert_eq!(dns_label("--"), "trestle");
    }
}
