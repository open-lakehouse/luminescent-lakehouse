//! The Tauri layer of the desktop shell: window, commands and events over `shell-core`.
//!
//! Kept thin on purpose: anything testable without a webview lives in `shell-core`.

mod commands;
mod run;
mod workspace;

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use shell_core::directory::CurrentDirectory;
use shell_core::services::{Proxy, SERVICES_VAR, StaticDirectory};
use shell_core::settings::SettingsFile;
use shell_core::toolchain;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_specta::{Builder, Event, collect_commands, collect_events};

/// How long a `--smoke` run may take before it counts as failed.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(90);
/// With `LUMINESCENT_SMOKE_DOCKER=start|resume` the smoke test also runs environments on
/// Docker (pulling images on a first run), so it gets longer.
const SMOKE_DOCKER_TIMEOUT: Duration = Duration::from_secs(900);
const SMOKE_DOCKER_VAR: &str = "LUMINESCENT_SMOKE_DOCKER";
/// With `LUMINESCENT_SMOKE_NEON=<branch>` the smoke test also uses that Neon branch and
/// checks the views show its data (`just desktop-demo-seed`); cold starts take a while.
const SMOKE_NEON_TIMEOUT: Duration = Duration::from_secs(180);
const SMOKE_NEON_VAR: &str = "LUMINESCENT_SMOKE_NEON";
/// Knob overrides for the Docker smoke's environment, `module.knob=value,…` (e.g. a
/// local image when the registry can't be reached).
const SMOKE_KNOBS_VAR: &str = "LUMINESCENT_SMOKE_KNOBS";
/// With `1`, the Docker smoke's second revision adds headwaters and checks the
/// Lineage view against it.
const SMOKE_LINEAGE_VAR: &str = "LUMINESCENT_SMOKE_LINEAGE";
/// Where the app's settings live, instead of its data directory.
const SETTINGS_DIR_VAR: &str = "LUMINESCENT_SETTINGS_DIR";

/// Shared state behind the commands.
pub struct AppState {
    proxy: Proxy,
    inflight: commands::Inflight,
    /// Log streams, by the UI's stream id, so the viewer can stop one.
    logs: commands::Inflight,
    /// Started with `--smoke`: the UI runs its smoke checks and reports back.
    smoke: bool,
    /// trestle's shared state store (`$TRESTLE_HOME/state.db`), shared with the CLI.
    store: workspace::SharedStore,
    /// Starts and stops instances on Docker, as the CLI does.
    runner: run::SharedRunner,
    /// What the proxy resolves modules against: bound by hand, then the current
    /// environment's.
    directory: Arc<CurrentDirectory>,
    /// Lists Neon branches and binds their functions.
    neon: run::SharedNeon,
    /// The current environment (a running instance or a Neon branch), if any.
    current: run::Current,
    /// What the launch's reconcile found, once it ran.
    reconciled: std::sync::Mutex<Option<shell_core::instances::ReconcileReport>>,
    /// Instances started from the app in this session.
    started: run::Started,
    /// The service windows open.
    windows: run::ServiceWindows,
}

/// The current environment changed (a started or chosen instance, a Neon branch, or
/// none after a stop); the UI rebinds its services, clears its query cache and remounts on `key`.
#[derive(Clone, Debug, Serialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentChanged {
    pub key: String,
    pub current: Option<shell_core::instances::CurrentEnvironment>,
}

impl EnvironmentChanged {
    /// `current` with its key: one per instance or branch, `none` without one.
    pub fn of(current: Option<shell_core::instances::CurrentEnvironment>) -> Self {
        EnvironmentChanged {
            key: current.as_ref().map_or_else(
                || "none".into(),
                shell_core::instances::CurrentEnvironment::key,
            ),
            current,
        }
    }
}

/// The commands and events, for both the app and the bindings export.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::doctor,
            commands::list_services,
            commands::service_fetch,
            commands::service_fetch_cancel,
            commands::smoke_report,
            workspace::state_location,
            workspace::state_workspaces,
            workspace::state_create_workspace,
            workspace::state_workspace,
            workspace::state_create_environment,
            workspace::state_delete_environment,
            workspace::state_set_selection,
            workspace::state_revisions,
            workspace::state_revision,
            workspace::state_commit,
            workspace::state_import,
            workspace::state_export_workspace,
            workspace::state_export_revision,
            workspace::pick_directory,
            workspace::planner_catalog,
            workspace::planner_plan,
            workspace::planner_bundle,
            run::run_workspace,
            run::run_start,
            run::run_stop,
            run::run_stop_instance,
            run::run_use,
            run::run_current,
            run::run_reconciled,
            run::run_service_uis,
            run::open_service_window,
            run::instance_logs,
            run::instance_logs_stop,
            run::settings_get,
            run::settings_set_quit,
            run::settings_set_one_at_a_time,
            run::neon_branches,
            run::neon_needs,
            run::neon_delete_branch,
            run::neon_reset_branch,
        ])
        .events(collect_events![
            EnvironmentChanged,
            workspace::StateChanged,
            run::InstanceChanged,
            run::Reconciled
        ])
}

/// Put the login shell's `PATH` ahead of the inherited one. Launched from the Finder, the app
/// has launchd's minimal `PATH`, without Homebrew, OrbStack or Docker Desktop.
fn adopt_login_shell_path() {
    let Some(login) = toolchain::login_shell_path() else {
        return;
    };
    let current = std::env::var("PATH").unwrap_or_default();
    // SAFETY: called first thing in `run`, before Tauri or any other thread starts.
    unsafe { std::env::set_var("PATH", toolchain::merge_path(&login, &current)) };
}

/// The stub directory (nothing is bound before instances exist, #18), unless
/// `LUMINESCENT_SERVICES` binds modules by hand.
fn service_directory() -> Arc<CurrentDirectory> {
    let overrides = match std::env::var(SERVICES_VAR) {
        Ok(spec) if !spec.trim().is_empty() => StaticDirectory::parse(&spec).unwrap_or_else(|e| {
            eprintln!("ignoring {SERVICES_VAR}: {e}");
            StaticDirectory::default()
        }),
        _ => StaticDirectory::default(),
    };
    Arc::new(CurrentDirectory::new(overrides))
}

pub fn run() {
    adopt_login_shell_path();
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let builder = specta_builder();
    let store = workspace::open_store();
    let directory = service_directory();
    let neon = run::neon_for(&store);
    if let Err(e) = &store {
        eprintln!("the shared state store is unavailable: {e}");
    }

    tauri::Builder::default()
        .manage(AppState {
            proxy: Proxy::new(directory.clone()),
            directory,
            current: Default::default(),
            reconciled: Default::default(),
            started: Default::default(),
            windows: Default::default(),
            inflight: Default::default(),
            logs: Default::default(),
            smoke,
            runner: run::runner_for(&store, &neon),
            neon,
            store: store.clone(),
        })
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            // LUMINESCENT_SETTINGS_DIR keeps a smoke run off the user's settings.
            let settings_dir = match std::env::var_os(SETTINGS_DIR_VAR) {
                Some(dir) => dir.into(),
                None => app.path().app_data_dir()?,
            };
            app.manage::<run::SharedSettings>(Arc::new(SettingsFile::load(&settings_dir)));
            workspace::watch(app.handle().clone(), &store);
            let runner = app.state::<AppState>().runner.clone();
            run::watch_instances(app.handle().clone(), &runner);
            run::reconcile_at_launch(app.handle().clone(), &runner);
            // The window is built here rather than in tauri.conf.json, so a smoke run can
            // flag itself to the UI before any script runs.
            let mut window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Luminescent Lakehouse")
                .inner_size(1280.0, 820.0)
                .min_inner_size(900.0, 600.0);
            if smoke {
                // `start` runs revisions side by side and leaves them running; `resume`
                // (a second launch) reconciles and stops them (scripts/smoke.sh runs both).
                let docker_phase = std::env::var(SMOKE_DOCKER_VAR)
                    .ok()
                    .filter(|p| p == "start" || p == "resume");
                let docker = docker_phase.is_some();
                let knobs = std::env::var(SMOKE_KNOBS_VAR).unwrap_or_default();
                let lineage = std::env::var(SMOKE_LINEAGE_VAR).is_ok_and(|v| v == "1");
                let neon = std::env::var(SMOKE_NEON_VAR)
                    .ok()
                    .filter(|b| !b.trim().is_empty());
                window = window.initialization_script(format!(
                    "window.__LL_SMOKE__ = true; window.__LL_SMOKE_DOCKER__ = {}; \
                     window.__LL_SMOKE_KNOBS__ = {}; window.__LL_SMOKE_LINEAGE__ = {lineage}; \
                     window.__LL_SMOKE_NEON__ = {};",
                    serde_json::to_string(&docker_phase).expect("a string serializes"),
                    serde_json::to_string(&knobs).expect("a string serializes"),
                    serde_json::to_string(&neon).expect("a string serializes"),
                ));
                let timeout = if docker {
                    SMOKE_DOCKER_TIMEOUT
                } else if neon.is_some() {
                    SMOKE_NEON_TIMEOUT
                } else {
                    SMOKE_TIMEOUT
                };
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(timeout);
                    eprintln!("smoke: FAIL no report within {}s", timeout.as_secs());
                    handle.exit(2);
                });
            }
            window.build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the desktop shell")
        .run(|app, event| {
            // Stop on quit: the exit waits for the stops (the second request goes through).
            if let tauri::RunEvent::ExitRequested { api, .. } = event
                && run::stop_on_quit(app)
            {
                api.prevent_exit();
            }
        });
}

#[cfg(test)]
mod tests {
    /// Regenerates `src/host/bindings.ts`. CI runs the tests and fails if it changed.
    #[test]
    fn export_bindings() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/host/bindings.ts");
        super::specta_builder()
            .export(
                specta_typescript::Typescript::default()
                    .header("// Generated by src-tauri's `export_bindings` test; do not edit."),
                path,
            )
            .expect("export the TypeScript bindings");
    }
}
