//! Commands that start, stop and report instances (shell-core's `instances`), on
//! the `docker` the doctor finds, sharing the instances with the `trestle` CLI.
//!
//! A started (or chosen) running instance becomes the current environment: the
//! service directory binds what its gateway exposes, and `environment-changed`
//! tells the UI, which remounts its views on the new key. A Neon branch can be the
//! current environment instead (shell-core's `neon`): its functions are bound.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use olai_stack_runtime::DockerCli;
use olai_stack_runtime::neon::NeonCli;
use serde::Serialize;
use shell_core::directory::CurrentDirectory;
use shell_core::instances::{
    CurrentEnvironment, InstanceChange, InstanceStatus, ReconcileReport, RunError, Runner,
    WorkspaceRun,
};
use shell_core::neon::{NeonBranches, NeonEnvironments, PROJECT_VAR};
use shell_core::settings::{QuitPolicy, Settings, SettingsFile};
use shell_core::state::SharedState;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::{AppState, EnvironmentChanged};

/// How long to wait before following Docker's events again after the stream ended
/// (the daemon stopped, or wasn't up yet).
const EVENTS_RETRY: Duration = Duration::from_secs(5);

/// A container of an instance changed (any process's instance); the Run view
/// refreshes that workspace.
#[derive(Clone, Debug, Serialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct InstanceChanged {
    pub change: InstanceChange,
}

/// The launch's reconcile finished ([`run_reconciled`] has its report).
#[derive(Clone, Debug, Serialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct Reconciled {
    pub report: ReconcileReport,
}

/// The app's settings.
pub type SharedSettings = Arc<SettingsFile>;

/// Instances started from the app in this session: what Stop on quit stops.
pub type Started = Mutex<BTreeSet<String>>;

/// The runner, or why there is none (the shared state didn't open).
pub type SharedRunner = Result<Arc<Runner<DockerCli>>, RunError>;

/// The current environment, if any.
pub type Current = Mutex<Option<CurrentEnvironment>>;

pub fn runner_for(store: &Result<Arc<SharedState>, shell_core::state::StateError>) -> SharedRunner {
    match store {
        Ok(state) => Ok(Arc::new(Runner::docker(state.store(), state.home()))),
        Err(e) => Err(RunError::Other {
            message: e.to_string(),
        }),
    }
}

/// Neon branches as environments, or why there are none (no shared state, no Neon CLI).
pub type SharedNeon = Result<Arc<NeonEnvironments>, RunError>;

pub fn neon_for(store: &Result<Arc<SharedState>, shell_core::state::StateError>) -> SharedNeon {
    let state = store.as_ref().map_err(|e| RunError::Other {
        message: e.to_string(),
    })?;
    let cli = NeonCli::new()?;
    Ok(Arc::new(NeonEnvironments::new(
        state.store(),
        Arc::new(cli),
        std::env::var(PROJECT_VAR).ok(),
    )))
}

fn announce(app: &AppHandle, current: Option<CurrentEnvironment>) {
    let _ = EnvironmentChanged::of(current).emit(app);
}

/// Remember the current instance (or that there is none) for the next launch.
fn remember(app: &AppHandle, instance: Option<String>) {
    let settings = app.state::<SharedSettings>();
    if let Err(e) = settings.update(|s| s.last_instance = instance) {
        eprintln!("{e}");
    }
}

/// Make `instance_id` the current environment: bind its gateway's services.
fn use_instance(
    app: &AppHandle,
    runner: &Runner<DockerCli>,
    directory: &CurrentDirectory,
    current: &Current,
    instance_id: &str,
) -> Result<CurrentEnvironment, RunError> {
    let roots = runner.bindings(instance_id)?;
    let described = runner.describe(instance_id)?;
    directory
        .set_local(&roots)
        .map_err(|message| RunError::Other { message })?;
    *current.lock().expect("current lock") = Some(described.clone());
    remember(app, Some(instance_id.to_owned()));
    announce(app, Some(described.clone()));
    Ok(described)
}

/// Follow Docker's events for every instance, for as long as the app runs, and
/// emit `instance-changed` for each; resume after the stream ends.
pub fn watch_instances(app: AppHandle, runner: &SharedRunner) {
    let Ok(runner) = runner.clone() else { return };
    tauri::async_runtime::spawn(async move {
        loop {
            if let Ok(changes) = runner.changes().await {
                let mut changes = Box::pin(changes);
                while let Some(change) = changes.next().await {
                    let _ = InstanceChanged { change }.emit(&app);
                }
            }
            tokio::time::sleep(EVENTS_RETRY).await;
        }
    });
}

/// At launch: match Docker against the store, then make the last current instance
/// current again if it still runs.
pub fn reconcile_at_launch(app: AppHandle, runner: &SharedRunner) {
    let Ok(runner) = runner.clone() else { return };
    tauri::async_runtime::spawn(async move {
        let report = match runner.reconcile().await {
            Ok(report) => report,
            Err(RunError::Unavailable { .. }) => ReconcileReport::default(),
            Err(e) => {
                eprintln!("reconcile: {e}");
                ReconcileReport::default()
            }
        };
        let state = app.state::<AppState>();
        *state.reconciled.lock().expect("reconciled lock") = Some(report.clone());
        let last = app.state::<SharedSettings>().get().last_instance;
        if let Some(last) = last {
            let none_yet = state.current.lock().expect("current lock").is_none();
            if none_yet && runner.is_running(&last).await.unwrap_or(false) {
                let _ = use_instance(&app, &runner, &state.directory, &state.current, &last);
            }
        }
        let _ = Reconciled { report }.emit(&app);
    });
}

/// With Stop on quit: stop the instances started in this session, then exit. Returns
/// whether it took over the exit (the caller then prevents it).
pub fn stop_on_quit(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    if app.state::<SharedSettings>().get().quit != QuitPolicy::Stop {
        return false;
    }
    let started: Vec<String> = std::mem::take(&mut *state.started.lock().expect("started lock"))
        .into_iter()
        .collect();
    let Ok(runner) = state.runner.clone() else {
        return false;
    };
    if started.is_empty() {
        return false;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for id in started {
            if runner.is_running(&id).await.unwrap_or(true)
                && let Err(e) = runner.stop_instance(&id).await
            {
                eprintln!("stopping {id} on quit: {e}");
            }
        }
        app.exit(0);
    });
    true
}

/// The workspace's environments with their instances and containers.
#[tauri::command]
#[specta::specta]
pub async fn run_workspace(
    state: State<'_, AppState>,
    workspace: String,
) -> Result<WorkspaceRun, RunError> {
    state.runner.clone()?.workspace(&workspace).await
}

/// Start the environment's head revision, wait for it to be healthy, and make it the
/// current environment.
#[tauri::command]
#[specta::specta]
pub async fn run_start(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: State<'_, SharedSettings>,
    environment: String,
) -> Result<InstanceStatus, RunError> {
    let runner = state.runner.clone()?;
    let workspace = runner.workspace_of(&environment)?;
    let one_at_a_time = settings.get().one_at_a_time.contains(&workspace);
    let started = runner.start(&environment, one_at_a_time).await?;
    state
        .started
        .lock()
        .expect("started lock")
        .insert(started.id.clone());
    use_instance(&app, &runner, &state.directory, &state.current, &started.id)?;
    Ok(started)
}

/// Make a running instance the current environment.
#[tauri::command]
#[specta::specta]
pub async fn run_use(
    app: AppHandle,
    state: State<'_, AppState>,
    instance: String,
) -> Result<CurrentEnvironment, RunError> {
    let runner = state.runner.clone()?;
    use_instance(&app, &runner, &state.directory, &state.current, &instance)
}

/// The current environment, if any, with the key the UI remounts on: what
/// `environment-changed` carries, so the UI never builds a key itself.
#[tauri::command]
#[specta::specta]
pub fn run_current(state: State<'_, AppState>) -> EnvironmentChanged {
    EnvironmentChanged::of(state.current.lock().expect("current lock").clone())
}

/// Stop the environment's instances (data volumes kept). If the current environment
/// was one of them, there is none any more.
#[tauri::command]
#[specta::specta]
pub async fn run_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    environment: String,
) -> Result<(), RunError> {
    state.runner.clone()?.stop(&environment).await?;
    let cleared = {
        let mut current = state.current.lock().expect("current lock");
        if current
            .as_ref()
            .is_some_and(|c| c.is_instance_of(&environment))
        {
            *current = None;
            true
        } else {
            false
        }
    };
    if cleared {
        state.directory.clear();
        remember(&app, None);
        announce(&app, None);
    }
    Ok(())
}

/// What the launch's reconcile found, once it ran (`reconciled` says when).
#[tauri::command]
#[specta::specta]
pub fn run_reconciled(state: State<'_, AppState>) -> Option<ReconcileReport> {
    state.reconciled.lock().expect("reconciled lock").clone()
}

#[tauri::command]
#[specta::specta]
pub fn settings_get(settings: State<'_, SharedSettings>) -> Settings {
    settings.get()
}

/// Keep the app's instances running on quit, or stop them.
#[tauri::command]
#[specta::specta]
pub fn settings_set_quit(
    settings: State<'_, SharedSettings>,
    quit: QuitPolicy,
) -> Result<Settings, String> {
    settings.update(|s| s.quit = quit)
}

/// Start the workspace's environments one at a time, or side by side.
#[tauri::command]
#[specta::specta]
pub fn settings_set_one_at_a_time(
    settings: State<'_, SharedSettings>,
    workspace: String,
    on: bool,
) -> Result<Settings, String> {
    settings.update(|s| {
        if on {
            s.one_at_a_time.insert(workspace);
        } else {
            s.one_at_a_time.remove(&workspace);
        }
    })
}

/// The workspace's Neon project (`trestle ws set-neon`, else the default) and its
/// branches.
#[tauri::command]
#[specta::specta]
pub async fn neon_branches(
    state: State<'_, AppState>,
    workspace: String,
) -> Result<NeonBranches, RunError> {
    state.neon.clone()?.branches(&workspace).await
}

/// Make a Neon branch the current environment: bind its functions.
#[tauri::command]
#[specta::specta]
pub async fn neon_use(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace: String,
    branch: String,
) -> Result<CurrentEnvironment, RunError> {
    let (described, roots) = state.neon.clone()?.bind(&workspace, &branch).await?;
    state
        .directory
        .set_remote(&roots)
        .map_err(|message| RunError::Other { message })?;
    *state.current.lock().expect("current lock") = Some(described.clone());
    remember(&app, None);
    announce(&app, Some(described.clone()));
    Ok(described)
}
