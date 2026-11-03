//! The Tauri layer of the desktop shell: window, commands and events over `shell-core`.
//!
//! Kept thin on purpose: anything testable without a webview lives in `shell-core`.

mod commands;
mod workspace;

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use shell_core::services::{Proxy, SERVICES_VAR, ServiceDirectory, StaticDirectory, StubDirectory};
use shell_core::toolchain;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tauri_specta::{Builder, Event, collect_commands, collect_events};

/// How long a `--smoke` run may take before it counts as failed.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(90);

/// Shared state behind the commands.
pub struct AppState {
    proxy: Proxy,
    inflight: commands::Inflight,
    /// Started with `--smoke`: the UI runs its smoke checks and reports back.
    smoke: bool,
    /// trestle's shared state store (`$TRESTLE_HOME/state.db`), shared with the CLI.
    store: workspace::SharedStore,
}

/// The current environment changed; the UI clears its query cache and remounts on `key`.
/// Emitted once environments exist (#17).
#[derive(Clone, Debug, Serialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentChanged {
    pub key: String,
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
fn service_directory() -> Arc<dyn ServiceDirectory> {
    match std::env::var(SERVICES_VAR) {
        Ok(spec) if !spec.trim().is_empty() => match StaticDirectory::parse(&spec) {
            Ok(dir) => Arc::new(dir),
            Err(e) => {
                eprintln!("ignoring {SERVICES_VAR}: {e}");
                Arc::new(StubDirectory)
            }
        },
        _ => Arc::new(StubDirectory),
    }
}

pub fn run() {
    adopt_login_shell_path();
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let builder = specta_builder();
    let store = workspace::open_store();
    if let Err(e) = &store {
        eprintln!("the shared state store is unavailable: {e}");
    }

    tauri::Builder::default()
        .manage(AppState {
            proxy: Proxy::new(service_directory()),
            inflight: Default::default(),
            smoke,
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
                window = window.initialization_script("window.__LL_SMOKE__ = true;");
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(SMOKE_TIMEOUT);
                    eprintln!("smoke: FAIL no report within {}s", SMOKE_TIMEOUT.as_secs());
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
