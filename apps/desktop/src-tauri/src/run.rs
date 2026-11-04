//! Commands that start, stop and report instances (shell-core's `instances`), on
//! the `docker` the doctor finds, sharing the instances with the `trestle` CLI.
//!
//! A started (or chosen) running instance becomes the current environment: the
//! service directory binds what its gateway exposes, and `environment-changed`
//! tells the UI, which remounts its views on the new key. A Neon branch can be the
//! current environment instead (shell-core's `neon`): its functions are bound.

use std::sync::{Arc, Mutex};

use olai_stack_runtime::DockerCli;
use olai_stack_runtime::neon::NeonCli;
use shell_core::directory::CurrentDirectory;
use shell_core::instances::{CurrentEnvironment, InstanceStatus, RunError, Runner, WorkspaceRun};
use shell_core::neon::{NeonBranches, NeonEnvironments, PROJECT_VAR};
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
    announce(&app, Some(described.clone()));
    Ok(described)
}
