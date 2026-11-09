//! Commands that start, stop and report instances (shell-core's `instances`), on
//! the `docker` the doctor finds, sharing the instances with the `trestle` CLI.
//!
//! A started (or chosen) running instance becomes the current environment: the
//! service directory binds what its gateway exposes and, for an instance on a Neon
//! branch, the branch's functions (shell-core's `neon`); `environment-changed`
//! tells the UI, which remounts its views on the new key.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use futures::future::{AbortHandle, Abortable};
use olai_stack_runtime::neon::NeonCli;
use olai_stack_runtime::{DockerCli, KeychainProvider};
use serde::Serialize;
use shell_core::directory::CurrentDirectory;
use shell_core::instances::{
    CurrentEnvironment, InstanceChange, InstanceStatus, KEYCHAIN_SERVICE, LogEntry,
    ReconcileReport, RunError, Runner, ServiceUi, WorkspaceRun,
};
use shell_core::neon::{
    BranchChoice, NeonBranch, NeonBranches, NeonEnvironments, NeonNeeds, NeonProject, NeonTarget,
    PROJECT_VAR, list_projects,
};
use shell_core::settings::{QuitPolicy, Settings, SettingsFile};
use shell_core::state::SharedState;
use shell_core::toolchain::HostSystem;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
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

/// At most this many log lines go in one message on the channel.
const LOG_BATCH: usize = 500;

/// One message on an instance's log channel.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LogMessage {
    Lines {
        lines: Vec<LogEntry>,
    },
    /// The containers stopped logging (they exited, or `follow` was off).
    End,
    Error {
        message: String,
    },
}

/// The app's settings.
pub type SharedSettings = Arc<SettingsFile>;

/// Instances started from the app in this session: what Stop on quit stops.
pub type Started = Mutex<BTreeSet<String>>;

/// The service windows open, by window label: the instance each shows.
pub type ServiceWindows = Mutex<std::collections::BTreeMap<String, String>>;

/// The runner, or why there is none (the shared state didn't open).
pub type SharedRunner = Result<Arc<Runner<DockerCli>>, RunError>;

/// The current environment, if any.
pub type Current = Mutex<Option<CurrentEnvironment>>;

pub fn runner_for(
    store: &Result<Arc<SharedState>, shell_core::state::StateError>,
    neon: &SharedNeon,
) -> SharedRunner {
    match store {
        Ok(state) => Ok(Arc::new(Runner::docker(
            state.store(),
            state.home(),
            neon.clone().ok(),
        ))),
        Err(e) => Err(RunError::Other {
            message: e.to_string(),
        }),
    }
}

/// The workspaces' Neon projects, or why there are none (no shared state, no Neon CLI).
pub type SharedNeon = Result<Arc<NeonEnvironments>, RunError>;

pub fn neon_for(store: &Result<Arc<SharedState>, shell_core::state::StateError>) -> SharedNeon {
    let state = store.as_ref().map_err(|e| RunError::Other {
        message: e.to_string(),
    })?;
    let cli = NeonCli::new()?;
    Ok(Arc::new(NeonEnvironments::new(
        state.store(),
        Arc::new(cli),
        Arc::new(KeychainProvider::new(KEYCHAIN_SERVICE)),
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

/// Make `instance_id` the current environment: bind its gateway's services and the
/// functions of the Neon branch it runs on.
async fn use_instance(
    app: &AppHandle,
    runner: &Runner<DockerCli>,
    directory: &CurrentDirectory,
    current: &Current,
    instance_id: &str,
) -> Result<CurrentEnvironment, RunError> {
    let roots = runner.bindings(instance_id)?;
    let functions = runner.function_roots(instance_id).await?;
    let described = runner.describe(instance_id)?;
    directory
        .set_instance(&roots, &functions)
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
                let _ = use_instance(&app, &runner, &state.directory, &state.current, &last).await;
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
/// current environment. On its Neon target, if it has one (`neon_target_set`), as
/// `trestle up --neon-branch` binds it; a target asking for a new branch becomes the
/// branch made for it, so the next Start reuses that.
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
    let current = settings.get();
    let one_at_a_time = current.one_at_a_time.contains(&workspace);
    let target = current.neon_targets.get(&environment).cloned();
    let started = runner
        .start(&environment, one_at_a_time, target.as_ref())
        .await?;
    if let (Some(target), Some(bound)) = (&target, &started.branch)
        && matches!(target.branch, BranchChoice::New { .. })
        && let Err(e) = settings.update(|s| {
            s.neon_targets.insert(
                environment.clone(),
                NeonTarget {
                    project: target.project.clone(),
                    branch: BranchChoice::Existing {
                        branch: bound.name.clone(),
                    },
                },
            );
        })
    {
        eprintln!("{e}");
    }
    state
        .started
        .lock()
        .expect("started lock")
        .insert(started.id.clone());
    use_instance(&app, &runner, &state.directory, &state.current, &started.id).await?;
    if one_at_a_time {
        close_stopped_windows(&app, &runner).await;
    }
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
    use_instance(&app, &runner, &state.directory, &state.current, &instance).await
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
    let runner = state.runner.clone()?;
    runner.stop(&environment).await?;
    close_stopped_windows(&app, &runner).await;
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

/// Stream an instance's logs to `on_lines`: the last `tail` lines per service, then
/// (with `follow`) new ones as they come, in batches. Stops at the end, or on
/// [`instance_logs_stop`] with the same `stream_id`.
#[tauri::command]
#[specta::specta]
pub async fn instance_logs(
    state: State<'_, AppState>,
    stream_id: u32,
    instance: String,
    tail: Option<u32>,
    follow: bool,
    on_lines: Channel<LogMessage>,
) -> Result<(), RunError> {
    let lines = state.runner.clone()?.logs(&instance, tail, follow).await?;
    let (handle, registration) = AbortHandle::new_pair();
    let streams = state.logs.clone();
    streams.lock().expect("logs lock").insert(stream_id, handle);
    tauri::async_runtime::spawn(async move {
        let pump = async {
            let mut batches = Box::pin(lines.ready_chunks(LOG_BATCH));
            while let Some(batch) = batches.next().await {
                let mut lines = Vec::with_capacity(batch.len());
                for line in batch {
                    match line {
                        Ok(line) => lines.push(line),
                        Err(message) => {
                            let _ = on_lines.send(LogMessage::Error { message });
                            return;
                        }
                    }
                }
                // The webview is gone (or reloaded): stop reading.
                if on_lines.send(LogMessage::Lines { lines }).is_err() {
                    return;
                }
            }
            let _ = on_lines.send(LogMessage::End);
        };
        let _ = Abortable::new(pump, registration).await;
        streams.lock().expect("logs lock").remove(&stream_id);
    });
    Ok(())
}

/// Stop streaming the logs of `stream_id` (the viewer closed).
#[tauri::command]
#[specta::specta]
pub fn instance_logs_stop(state: State<'_, AppState>, stream_id: u32) {
    if let Some(handle) = state.logs.lock().expect("logs lock").remove(&stream_id) {
        handle.abort();
    }
}

/// The instance's service UIs that open in their own window (MLflow, Jaeger, marimo,
/// Omnigent), on its own host name.
#[tauri::command]
#[specta::specta]
pub fn run_service_uis(
    state: State<'_, AppState>,
    instance: String,
) -> Result<Vec<ServiceUi>, RunError> {
    state.runner.clone()?.service_uis(&instance)
}

/// Open (or focus) the window of one service UI of the instance: a top-level webview
/// on `<host label>.localhost`, which keeps the UI's cookie session apart from other
/// instances'. It is a page of the service, not of the app: a remote origin, which
/// no capability lists under `remote`, so it gets no IPC access.
#[tauri::command]
#[specta::specta]
pub fn open_service_window(
    app: AppHandle,
    state: State<'_, AppState>,
    instance: String,
    module: String,
) -> Result<(), RunError> {
    let runner = state.runner.clone()?;
    let ui = runner
        .service_uis(&instance)?
        .into_iter()
        .find(|u| u.module_id == module)
        .ok_or_else(|| RunError::Other {
            message: format!("the instance has no {module} UI"),
        })?;
    let label = window_label(&instance, &module);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        return window.set_focus().map_err(other);
    }
    let title = match runner.describe(&instance)? {
        CurrentEnvironment::Instance {
            workspace,
            environment,
            revision,
            ..
        } => format!("{} · {workspace}/{environment} #{revision}", ui.name),
    };
    let url = ui.url.parse().map_err(other)?;
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url))
        .title(title)
        .inner_size(1280.0, 860.0)
        .build()
        .map_err(other)?;
    state
        .windows
        .lock()
        .expect("windows lock")
        .insert(label, instance);
    Ok(())
}

fn other(e: impl std::fmt::Display) -> RunError {
    RunError::Other {
        message: e.to_string(),
    }
}

/// `svc-<instance>-<module>`, in the characters a window label allows.
fn window_label(instance: &str, module: &str) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    };
    format!("svc-{}-{}", clean(instance), clean(module))
}

/// Close the service windows of instances that no longer run (after a stop, or a
/// one-at-a-time start).
async fn close_stopped_windows(app: &AppHandle, runner: &Runner<DockerCli>) {
    let open: Vec<(String, String)> = {
        let state = app.state::<AppState>();
        let windows = state.windows.lock().expect("windows lock");
        windows
            .iter()
            .map(|(l, i)| (l.clone(), i.clone()))
            .collect()
    };
    for (label, instance) in open {
        let running = runner.is_running(&instance).await.unwrap_or(true);
        let gone = app.get_webview_window(&label).is_none();
        if !running || gone {
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.close();
            }
            app.state::<AppState>()
                .windows
                .lock()
                .expect("windows lock")
                .remove(&label);
        }
    }
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

/// Stop one instance (data volumes kept), e.g. one of two revisions running side by
/// side. If it was the current environment, there is none any more.
#[tauri::command]
#[specta::specta]
pub async fn run_stop_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    instance: String,
) -> Result<(), RunError> {
    let runner = state.runner.clone()?;
    runner.stop_instance(&instance).await?;
    close_stopped_windows(&app, &runner).await;
    let cleared = {
        let mut current = state.current.lock().expect("current lock");
        let is_it = matches!(
            current.as_ref(),
            Some(CurrentEnvironment::Instance { instance_id, .. }) if *instance_id == instance
        );
        if is_it {
            *current = None;
        }
        is_it
    };
    if cleared {
        state.directory.clear();
        remember(&app, None);
        announce(&app, None);
    }
    Ok(())
}

/// The workspace's Neon project (`trestle ws set-neon`, else the default) and its
/// branches.
#[tauri::command]
#[specta::specta]
pub async fn neon_branches(
    state: State<'_, AppState>,
    workspace: String,
    project: Option<String>,
) -> Result<NeonBranches, RunError> {
    state
        .neon
        .clone()?
        .branches(&workspace, project.as_deref())
        .await
}

/// Make a branch now (in `project`, else the workspace's, from its parent), owned by
/// trestle and expiring after `ttl_hours`: an environment's target can then point at it.
#[tauri::command]
#[specta::specta]
pub async fn neon_create_branch(
    state: State<'_, AppState>,
    workspace: String,
    project: Option<String>,
    name: String,
    ttl_hours: Option<u32>,
) -> Result<NeonBranch, RunError> {
    state
        .neon
        .clone()?
        .create_branch(&workspace, project.as_deref(), &name, ttl_hours)
        .await
}

/// Every project of the organizations the Neon CLI is signed in to: what an
/// environment's target can point at.
#[tauri::command]
#[specta::specta]
pub async fn neon_projects() -> Result<Vec<NeonProject>, RunError> {
    tauri::async_runtime::spawn_blocking(|| list_projects(&HostSystem))
        .await
        .map_err(other)?
}

/// Set (or clear) where an environment runs on Neon: what its next Start binds.
#[tauri::command]
#[specta::specta]
pub fn neon_target_set(
    settings: State<'_, SharedSettings>,
    environment: String,
    target: Option<NeonTarget>,
) -> Result<Settings, String> {
    settings.update(|s| match target {
        Some(target) => {
            s.neon_targets.insert(environment, target);
        }
        None => {
            s.neon_targets.remove(&environment);
        }
    })
}

/// What the environment's head revision needs of a Neon branch: whether Start asks
/// for one.
#[tauri::command]
#[specta::specta]
pub fn neon_needs(state: State<'_, AppState>, environment: String) -> Result<NeonNeeds, RunError> {
    state.runner.clone()?.neon_needs(&environment)
}

/// Delete a branch trestle created, once no running instance is on it: the stopped
/// ones are unbound first (their storage credentials revoked), and environments that
/// last started on it forget it.
#[tauri::command]
#[specta::specta]
pub async fn neon_delete_branch(
    state: State<'_, AppState>,
    settings: State<'_, SharedSettings>,
    workspace: String,
    branch: String,
) -> Result<NeonBranch, RunError> {
    let neon = state.neon.clone()?;
    let found = neon.find(&workspace, &branch).await?;
    if !found.owned {
        return Err(RunError::Neon {
            message: format!(
                "`{}` wasn't created by trestle, so it isn't deleted here",
                found.name
            ),
        });
    }
    state
        .runner
        .clone()?
        .release_branch(&workspace, &found.id)
        .await?;
    let deleted = neon.delete_branch(&workspace, &found.id).await?;
    // Environments that pointed at it point nowhere now (on the workspace's project).
    let gone = |t: &NeonTarget| {
        t.project.is_none()
            && matches!(&t.branch, BranchChoice::Existing { branch } if *branch == deleted.name || *branch == deleted.id)
    };
    if let Err(e) = settings.update(|s| s.neon_targets.retain(|_, t| !gone(t))) {
        eprintln!("{e}");
    }
    Ok(deleted)
}

/// Reset a branch trestle created to its parent's data.
#[tauri::command]
#[specta::specta]
pub async fn neon_reset_branch(
    state: State<'_, AppState>,
    workspace: String,
    branch: String,
) -> Result<(), RunError> {
    state.neon.clone()?.reset_branch(&workspace, &branch).await
}
