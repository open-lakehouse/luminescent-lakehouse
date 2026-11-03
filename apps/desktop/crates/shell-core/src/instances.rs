//! Running environments: an environment's head revision as a Docker Compose
//! instance, through trestle's `olai-stack-runtime`, as `trestle up` / `trestle down`
//! run them. The app and the CLI share the instances (the store records them and
//! their slots), so either can stop what the other started.
//!
//! What runs comes from Docker (`compose ps`), as `trestle ps` / `trestle health`
//! report it; the store only records what is wanted.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use olai_stack_runtime::neon::{NeonBranchSource, NeonCli};
use olai_stack_runtime::{
    ComposeEngine, DockerCli, EnvProvider, Health, InstanceError, Instances, KeychainProvider,
    SecretProviders, ServiceStatus, State, instance_project, remove_mesh_if_idle,
};
use olai_stack_state::{Desired, EnvironmentId, Instance, Store, WorkspaceId};
use olai_stack_topology::manifest::EnvManifest;
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
    pub async fn start(&self, env_id: &str) -> Result<InstanceStatus> {
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
        self.runtime.start(&instance.id).await?;
        self.instance_status(&self.store.instance(&instance.id)?, true)
            .await
    }

    /// Stop the environment's instances that are wanted running or still have
    /// containers (`compose down`, data volumes kept), then drop the workspace's mesh
    /// network if no other instance uses it.
    pub async fn stop(&self, env_id: &str) -> Result<()> {
        let env = self.store.environment(&Self::env_id(env_id))?;
        let workspace = self.store.workspace_by_id(&env.workspace)?;
        let projects = self.running_projects().await?;
        for instance in self.store.instances_of(&env.id)? {
            let project = instance_project(&workspace.name, &env.name, instance.revision);
            if instance.desired == Desired::Running || projects.contains(&project) {
                self.runtime.stop(&instance.id, false).await?;
            }
        }
        if workspace.mesh {
            let members = self.workspace_projects(&workspace.id)?;
            let network = format!("trestle-{}-mesh", workspace.name);
            remove_mesh_if_idle(self.runtime.engine(), &network, &members)
                .await
                .map_err(engine_error)?;
        }
        Ok(())
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

    /// The gateway's URL from the host, planned from the revision at the instance's
    /// placement (without secrets, which the URL doesn't depend on).
    fn gateway_url(&self, instance: &Instance, env_name: &str) -> Result<Option<String>> {
        let revision = self
            .store
            .revision(&instance.environment, instance.revision)?;
        let placement = self.runtime.placement(&instance.id)?;
        let manifest: &EnvManifest = revision.manifest();
        let mut ctx = manifest.context.clone();
        ctx.env_name = env_name.to_owned();
        placement.apply(&mut ctx);
        let mut catalog = baseline_catalog();
        for text in revision.modules().values() {
            if let Ok(custom) = Catalog::from_manifests([text.as_str()]) {
                catalog = catalog.merge(custom);
            }
        }
        Ok(catalog
            .plan(&manifest.selection, &ctx)
            .ok()
            .and_then(|plan| plan.gateway_url()))
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
            f.runner.start(&f.dev).await,
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

        let started = f.runner.start(&f.dev).await.unwrap();
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
        let again = f.runner.start(&f.dev).await.unwrap();
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
        let started = f.runner.start(&f.dev).await.unwrap();
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
            f.runner.start(&f.dev).await,
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
        f.runner.start(&f.dev).await.unwrap();
        f.engine.set_unavailable(true);
        let run = f.runner.workspace(&f.workspace).await.unwrap();
        assert!(run.engine_problem.is_some());
        assert_eq!(run.environments[0].instances[0].desired, "running");
        assert!(matches!(
            f.runner.start(&f.dev).await,
            Err(RunError::Unavailable { .. })
        ));
    }

    #[tokio::test]
    async fn a_new_revision_starts_as_a_new_instance_on_its_own_slot() {
        let f = fixture();
        f.state.commit(&f.dev).unwrap();
        f.runner.start(&f.dev).await.unwrap();
        // Change the draft and commit revision 2.
        let mut selection = f.state.workspace(&f.workspace).unwrap().environments[0]
            .selection
            .clone();
        selection.0["modules"].as_array_mut().unwrap().pop();
        f.state.set_selection(&f.dev, selection).unwrap();
        f.state.commit(&f.dev).unwrap();
        let second = f.runner.start(&f.dev).await.unwrap();
        assert_eq!((second.revision, second.slot), (2, Some(1)));
        assert_eq!(
            second.gateway_url.as_deref(),
            Some("http://localhost:10080")
        );
    }
}
