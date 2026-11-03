//! Commands that start, stop and report instances (shell-core's `instances`), on
//! the `docker` the doctor finds, sharing the instances with the `trestle` CLI.

use std::sync::Arc;

use olai_stack_runtime::DockerCli;
use shell_core::instances::{InstanceStatus, RunError, Runner, WorkspaceRun};
use shell_core::state::SharedState;
use tauri::State;

use crate::AppState;

/// The runner, or why there is none (the shared state didn't open).
pub type SharedRunner = Result<Arc<Runner<DockerCli>>, RunError>;

pub fn runner_for(store: &Result<Arc<SharedState>, shell_core::state::StateError>) -> SharedRunner {
    match store {
        Ok(state) => Ok(Arc::new(Runner::docker(state.store(), state.home()))),
        Err(e) => Err(RunError::Other {
            message: e.to_string(),
        }),
    }
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

/// Start the environment's head revision and wait for it to be healthy.
#[tauri::command]
#[specta::specta]
pub async fn run_start(
    state: State<'_, AppState>,
    environment: String,
) -> Result<InstanceStatus, RunError> {
    state.runner.clone()?.start(&environment).await
}

/// Stop the environment's instances (data volumes kept).
#[tauri::command]
#[specta::specta]
pub async fn run_stop(state: State<'_, AppState>, environment: String) -> Result<(), RunError> {
    state.runner.clone()?.stop(&environment).await
}
