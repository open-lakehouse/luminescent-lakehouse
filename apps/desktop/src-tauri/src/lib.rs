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
use shell_core::toolchain;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tauri_specta::{Builder, Event, collect_commands, collect_events};

/// How long a `--smoke` run may take before it counts as failed.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(90);
/// With `LUMINESCENT_SMOKE_DOCKER=1` the smoke test also starts an environment on
/// Docker (pulling images on a first run), so it gets longer.
const SMOKE_DOCKER_TIMEOUT: Duration = Duration::from_secs(900);
const SMOKE_DOCKER_VAR: &str = "LUMINESCENT_SMOKE_DOCKER";

/// Shared state behind the commands.
pub struct AppState {
    proxy: Proxy,
    inflight: commands::Inflight,
    /// Started with `--smoke`: the UI runs its smoke checks and reports back.
    smoke: bool,
    /// trestle's shared state store (`$TRESTLE_HOME/state.db`), shared with the CLI.
    store: workspace::SharedStore,
    /// Starts and stops instances on Docker, as the CLI does.
    runner: run::SharedRunner,
    /// What the proxy resolves modules against: bound by hand, then the current
    /// environment's.
    directory: Arc<CurrentDirectory>,
    /// The current environment (a running instance), if any.
    current: run::Current,
}

/// The current environment changed (a started or chosen instance, or none after a
/// stop); the UI rebinds its services, clears its query cache and remounts on `key`.
#[derive(Clone, Debug, Serialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentChanged {
    pub key: String,
    pub current: Option<shell_core::instances::CurrentEnvironment>,
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
            workspace::planner_catalog,
            workspace::planner_plan,
            workspace::planner_bundle,
            run::run_workspace,
            run::run_start,
            run::run_stop,
            run::run_use,
            run::run_current,
        ])
        .events(collect_events![EnvironmentChanged, workspace::StateChanged])
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
    if let Err(e) = &store {
        eprintln!("the shared state store is unavailable: {e}");
    }

    tauri::Builder::default()
        .manage(AppState {
            proxy: Proxy::new(directory.clone()),
            directory,
            current: Default::default(),
            inflight: Default::default(),
            smoke,
            runner: run::runner_for(&store),
            store: store.clone(),
        })
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            workspace::watch(app.handle().clone(), &store);
            // The window is built here rather than in tauri.conf.json, so a smoke run can
            // flag itself to the UI before any script runs.
            let mut window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Luminescent Lakehouse")
                .inner_size(1280.0, 820.0)
                .min_inner_size(900.0, 600.0);
            if smoke {
                let docker = std::env::var(SMOKE_DOCKER_VAR).is_ok_and(|v| v == "1");
                window = window.initialization_script(format!(
                    "window.__LL_SMOKE__ = true; window.__LL_SMOKE_DOCKER__ = {docker};"
                ));
                let timeout = if docker {
                    SMOKE_DOCKER_TIMEOUT
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
        .run(tauri::generate_context!())
        .expect("error while running the desktop shell");
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
