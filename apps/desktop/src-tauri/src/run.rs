//! Commands that start, stop and report instances (shell-core's `instances`), on
//! the `docker` the doctor finds, sharing the instances with the `trestle` CLI.
//!
//! A started (or chosen) running instance becomes the current environment: the
//! service directory binds what its gateway exposes, and `environment-changed`
//! tells the UI, which remounts its views on the new key.

use std::sync::{Arc, Mutex};

use olai_stack_runtime::DockerCli;
use shell_core::directory::CurrentDirectory;
use shell_core::instances::{CurrentEnvironment, InstanceStatus, RunError, Runner, WorkspaceRun};
use shell_core::state::SharedState;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{AppState, EnvironmentChanged};

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

/// The key the UI remounts on: one per instance.
pub fn key_of(current: Option<&CurrentEnvironment>) -> String {
    current.map_or_else(|| "none".into(), |c| format!("instance:{}", c.instance_id))
}

fn announce(app: &AppHandle, current: Option<CurrentEnvironment>) {
    let _ = EnvironmentChanged {
        key: key_of(current.as_ref()),
        current,
    }
    .emit(app);
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
    announce(app, Some(described.clone()));
    Ok(described)
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
    environment: String,
) -> Result<InstanceStatus, RunError> {
    let runner = state.runner.clone()?;
    let started = runner.start(&environment).await?;
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

/// The current environment, if any.
#[tauri::command]
#[specta::specta]
pub fn run_current(state: State<'_, AppState>) -> Option<CurrentEnvironment> {
    state.current.lock().expect("current lock").clone()
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
            .is_some_and(|c| c.environment_id == environment)
        {
            *current = None;
            true
        } else {
            false
        }
    };
    if cleared {
        state.directory.clear();
        announce(&app, None);
    }
    Ok(())
}
