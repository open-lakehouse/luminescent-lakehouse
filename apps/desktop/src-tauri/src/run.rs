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
use serde::{Deserialize, Serialize};
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
use tauri::webview::WebviewBuilder;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Rect, State, WebviewUrl, WebviewWindowBuilder,
};
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

/// The service UIs open, in their own window or embedded in the main one, by webview
/// label (`svc-…` for a window, `emb-…` for an embed): the instance each shows.
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

/// The instance's service UIs with a session of their own (MLflow, Jaeger, marimo,
/// Omnigent), on its own host name: embedded in the main window, or in their own.
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
/// no capability lists under `remote`, so it gets no IPC access. A UI shows in one
/// place at a time: its embed, if any, closes (this is the embed's Pop out).
#[tauri::command]
#[specta::specta]
pub fn open_service_window(
    app: AppHandle,
    state: State<'_, AppState>,
    instance: String,
    module: String,
) -> Result<(), RunError> {
    let runner = state.runner.clone()?;
    let ui = service_ui(&runner, &instance, &module)?;
    close_webview(&app, &state, &embed_label(&instance, &module));
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

/// Where an embedded service UI sits in the main window: a rectangle of the main
/// webview, in whole CSS pixels (the window's logical pixels, as the webview fills it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct EmbedBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl EmbedBounds {
    /// The bounds as a window rectangle, or `None` for an empty area (nothing to
    /// show: a collapsed or not yet laid out tab).
    fn visible(self) -> Option<Rect> {
        (self.width > 0 && self.height > 0).then(|| Rect {
            position: LogicalPosition::new(self.x, self.y).into(),
            size: LogicalSize::new(self.width, self.height).into(),
        })
    }
}

/// Whether service UIs can show inside the main window. Not on Linux: there tauri packs
/// a child webview into the window's `gtk::Box` beside the main one, where neither its
/// position nor its size can be set, so the UIs open in their own window instead.
#[tauri::command]
#[specta::specta]
pub fn service_embeds_supported() -> bool {
    EMBEDS_SUPPORTED
}

const EMBEDS_SUPPORTED: bool = !cfg!(target_os = "linux");

/// Show one service UI of the instance inside the main window, over `bounds` (the
/// tab's area), or hide it (`None`: the tab closed, or an overlay covers it). The
/// embed is a child webview of the main window: like the UI's own window, a
/// top-level page on `<host label>.localhost` (not a frame, so its cookies work),
/// a remote origin with no IPC access. Created on first use, then moved, shown and
/// hidden; a UI shows in one place at a time, so its window, if open, closes.
#[tauri::command]
#[specta::specta]
pub fn embed_service_ui(
    app: AppHandle,
    state: State<'_, AppState>,
    instance: String,
    module: String,
    bounds: Option<EmbedBounds>,
) -> Result<(), RunError> {
    if !EMBEDS_SUPPORTED {
        return Err(RunError::Other {
            message: "service UIs open in their own window on this platform".into(),
        });
    }
    let label = embed_label(&instance, &module);
    let existing = app.get_webview(&label);
    let Some(rect) = bounds.and_then(EmbedBounds::visible) else {
        if let Some(webview) = existing {
            webview.hide().map_err(other)?;
        }
        return Ok(());
    };
    if let Some(webview) = existing {
        webview.set_bounds(rect).map_err(other)?;
        return webview.show().map_err(other);
    }
    let runner = state.runner.clone()?;
    let ui = service_ui(&runner, &instance, &module)?;
    close_webview(&app, &state, &window_label(&instance, &module));
    let main = app.get_window(MAIN_WINDOW).ok_or_else(|| RunError::Other {
        message: "the main window is gone".into(),
    })?;
    let url = ui.url.parse().map_err(other)?;
    main.add_child(
        WebviewBuilder::new(&label, WebviewUrl::External(url)),
        rect.position,
        rect.size,
    )
    .map_err(other)?;
    state
        .windows
        .lock()
        .expect("windows lock")
        .insert(label, instance);
    Ok(())
}

/// An embedded service UI as the host placed it: what it loaded, and where.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct ServiceEmbed {
    pub url: String,
    pub bounds: EmbedBounds,
}

/// The instance's embedded `module` UI, if one exists (shown or hidden): for the
/// smoke run, which checks the real webview against the tab's area.
#[tauri::command]
#[specta::specta]
pub fn service_embed(
    app: AppHandle,
    instance: String,
    module: String,
) -> Result<Option<ServiceEmbed>, RunError> {
    let Some(webview) = app.get_webview(&embed_label(&instance, &module)) else {
        return Ok(None);
    };
    let scale = webview.window().scale_factor().map_err(other)?;
    let rect = webview.bounds().map_err(other)?;
    let position = rect.position.to_logical::<f64>(scale);
    let size = rect.size.to_logical::<f64>(scale);
    Ok(Some(ServiceEmbed {
        url: webview.url().map_err(other)?.to_string(),
        bounds: EmbedBounds {
            x: position.x.round() as i32,
            y: position.y.round() as i32,
            width: size.width.round() as u32,
            height: size.height.round() as u32,
        },
    }))
}

/// The cookies an embedded UI's page holds, by name: on its own host label, and
/// on another label at the same port (which must see none of them).
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EmbedCookies {
    pub own: Vec<String>,
    pub other_label: Vec<String>,
}

/// The smoke run's cookie probe of the instance's embedded `module` UI: with `set`,
/// its page first sets `ll_smoke_probe` from script. Then the cookie names its URL
/// has, and those of the same URL on another host label. Only under `--smoke`.
#[tauri::command]
#[specta::specta]
pub async fn smoke_embed_cookies(
    app: AppHandle,
    state: State<'_, AppState>,
    instance: String,
    module: String,
    set: bool,
) -> Result<EmbedCookies, RunError> {
    if !state.smoke {
        return Err(RunError::Other {
            message: "not running with --smoke".into(),
        });
    }
    let webview = app
        .get_webview(&embed_label(&instance, &module))
        .ok_or_else(|| RunError::Other {
            message: format!("no embedded {module} UI"),
        })?;
    if set {
        webview
            .eval("document.cookie = 'll_smoke_probe=1; path=/; max-age=3600'")
            .map_err(other)?;
    }
    let url = webview.url().map_err(other)?;
    let mut elsewhere = url.clone();
    elsewhere
        .set_host(Some("ll-smoke-other.localhost"))
        .map_err(other)?;
    let names = |url| -> Result<Vec<String>, RunError> {
        Ok(webview
            .cookies_for_url(url)
            .map_err(other)?
            .iter()
            .map(|c| c.name().to_owned())
            .collect())
    };
    Ok(EmbedCookies {
        own: names(url)?,
        other_label: names(elsewhere)?,
    })
}

/// The main window's label.
pub const MAIN_WINDOW: &str = "main";

/// Close every embedded service UI: the main page (re)loaded, so no tab shows one.
pub fn close_embeds(app: &AppHandle) {
    let state = app.state::<AppState>();
    let embeds: Vec<String> = {
        let windows = state.windows.lock().expect("windows lock");
        windows
            .keys()
            .filter(|l| l.starts_with(EMBED_PREFIX))
            .cloned()
            .collect()
    };
    for label in embeds {
        close_webview(app, &state, &label);
    }
}

/// The instance's UI of `module`.
fn service_ui(
    runner: &Runner<DockerCli>,
    instance: &str,
    module: &str,
) -> Result<ServiceUi, RunError> {
    runner
        .service_uis(instance)?
        .into_iter()
        .find(|u| u.module_id == module)
        .ok_or_else(|| RunError::Other {
            message: format!("the instance has no {module} UI"),
        })
}

/// Close the service UI labelled `label`, a window or an embed, if it is open.
fn close_webview(app: &AppHandle, state: &AppState, label: &str) {
    if label.starts_with(EMBED_PREFIX) {
        if let Some(webview) = app.get_webview(label) {
            let _ = webview.close();
        }
    } else if let Some(window) = app.get_webview_window(label) {
        let _ = window.close();
    }
    state.windows.lock().expect("windows lock").remove(label);
}

/// Whether the service UI labelled `label` is still open (a window can be closed by
/// hand).
fn is_open(app: &AppHandle, label: &str) -> bool {
    if label.starts_with(EMBED_PREFIX) {
        app.get_webview(label).is_some()
    } else {
        app.get_webview_window(label).is_some()
    }
}

fn other(e: impl std::fmt::Display) -> RunError {
    RunError::Other {
        message: e.to_string(),
    }
}

const WINDOW_PREFIX: &str = "svc-";
const EMBED_PREFIX: &str = "emb-";

/// `svc-<instance>-<module>`, in the characters a window label allows.
fn window_label(instance: &str, module: &str) -> String {
    ui_label(WINDOW_PREFIX, instance, module)
}

/// `emb-<instance>-<module>`, the label of the UI's embed.
fn embed_label(instance: &str, module: &str) -> String {
    ui_label(EMBED_PREFIX, instance, module)
}

fn ui_label(prefix: &str, instance: &str, module: &str) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    };
    format!("{prefix}{}-{}", clean(instance), clean(module))
}

/// Close the service windows and embeds of instances that no longer run (after a
/// stop, or a one-at-a-time start).
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
        if !running || !is_open(app, &label) {
            close_webview(app, &app.state::<AppState>(), &label);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: i32, y: i32, width: u32, height: u32) -> EmbedBounds {
        EmbedBounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn embed_bounds_are_a_logical_rectangle() {
        let rect = bounds(240, 73, 901, 600).visible().expect("visible");
        assert_eq!(rect.position, LogicalPosition::new(240, 73).into());
        assert_eq!(rect.size, LogicalSize::new(901, 600).into());
    }

    #[test]
    fn an_empty_area_shows_nothing() {
        assert!(bounds(0, 0, 0, 600).visible().is_none());
        assert!(bounds(0, 0, 800, 0).visible().is_none());
    }

    #[test]
    fn a_ui_has_a_window_label_and_an_embed_label() {
        assert_eq!(
            window_label("local/dev#3", "mlflow"),
            "svc-local-dev-3-mlflow"
        );
        assert_eq!(
            embed_label("local/dev#3", "mlflow"),
            "emb-local-dev-3-mlflow"
        );
    }
}
