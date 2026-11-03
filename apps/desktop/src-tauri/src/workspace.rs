//! Commands over the shared state store and the native planner (shell-core's
//! `state` and `planner`), and the `state-changed` event that tells the UI to
//! reload after any write, the CLI's included.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use serde::Serialize;
use shell_core::Json;
use shell_core::planner::{self, PlanError};
use shell_core::state::{
    EnvironmentDraft, RevisionInfo, SharedState, StateError, WATCH_INTERVAL, WorkspaceInfo,
    WorkspaceSnapshot,
};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::AppState;

/// The shared state store changed (any process's write, the app's own included).
#[derive(Clone, Debug, Serialize, specta::Type, Event)]
pub struct StateChanged;

/// The store, or why it couldn't be opened (the app still runs, its views say why).
pub type SharedStore = Result<Arc<SharedState>, StateError>;

pub fn open_store() -> SharedStore {
    SharedState::open(None).map(Arc::new)
}

/// Emit `state-changed` whenever the store changes.
pub fn watch(app: AppHandle, store: &SharedStore) {
    let Ok(store) = store else { return };
    let changes = match store.watch(WATCH_INTERVAL) {
        Ok(changes) => changes,
        Err(e) => {
            eprintln!("not watching the state store: {e}");
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        let mut changes = Box::pin(changes);
        while changes.next().await.is_some() {
            let _ = StateChanged.emit(&app);
        }
    });
}

/// Run `f` on the store, off the async runtime (the store's calls block).
async fn with_store<T: Send + 'static>(
    state: &AppState,
    f: impl FnOnce(&SharedState) -> Result<T, StateError> + Send + 'static,
) -> Result<T, StateError> {
    let store = state.store.clone()?;
    tauri::async_runtime::spawn_blocking(move || f(&store))
        .await
        .map_err(|e| StateError::Store {
            message: e.to_string(),
        })?
}

/// Where the store is, for the UI to show (`$TRESTLE_HOME/state.db`).
#[tauri::command]
#[specta::specta]
pub fn state_location(state: State<'_, AppState>) -> Result<String, StateError> {
    Ok(state.store.clone()?.path().display().to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn state_workspaces(
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceInfo>, StateError> {
    with_store(&state, |s| s.workspaces()).await
}

/// A new workspace with one environment (`first_env`) on the baseline selection.
#[tauri::command]
#[specta::specta]
pub async fn state_create_workspace(
    state: State<'_, AppState>,
    name: String,
    mesh: bool,
    first_env: String,
) -> Result<WorkspaceInfo, StateError> {
    with_store(&state, move |s| s.create_workspace(&name, mesh, &first_env)).await
}

#[tauri::command]
#[specta::specta]
pub async fn state_workspace(
    state: State<'_, AppState>,
    id: String,
) -> Result<WorkspaceSnapshot, StateError> {
    with_store(&state, move |s| s.workspace(&id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn state_create_environment(
    state: State<'_, AppState>,
    workspace: String,
    name: String,
    selection: Option<Json>,
) -> Result<EnvironmentDraft, StateError> {
    with_store(&state, move |s| {
        s.create_environment(&workspace, &name, selection)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn state_delete_environment(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), StateError> {
    with_store(&state, move |s| s.delete_environment(&id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn state_set_selection(
    state: State<'_, AppState>,
    id: String,
    selection: Json,
) -> Result<(), StateError> {
    with_store(&state, move |s| s.set_selection(&id, selection)).await
}

#[tauri::command]
#[specta::specta]
pub async fn state_revisions(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<RevisionInfo>, StateError> {
    with_store(&state, move |s| s.revisions(&id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn state_revision(
    state: State<'_, AppState>,
    id: String,
    number: u32,
) -> Result<Option<RevisionInfo>, StateError> {
    with_store(&state, move |s| s.revision(&id, number)).await
}

#[tauri::command]
#[specta::specta]
pub async fn state_commit(
    state: State<'_, AppState>,
    id: String,
) -> Result<RevisionInfo, StateError> {
    with_store(&state, move |s| s.commit(&id)).await
}

/// The editor's `Planner.catalog()`.
#[tauri::command]
#[specta::specta]
pub async fn planner_catalog() -> Json {
    tauri::async_runtime::spawn_blocking(planner::catalog)
        .await
        .expect("the catalog projection doesn't panic")
}

/// The editor's `Planner.plan()`.
#[tauri::command]
#[specta::specta]
pub async fn planner_plan(
    selection: Json,
    target: Option<String>,
    context: Option<Json>,
) -> Result<Json, PlanError> {
    tauri::async_runtime::spawn_blocking(move || {
        planner::plan(selection, target.as_deref(), context)
    })
    .await
    .map_err(|e| PlanError::Plan {
        message: e.to_string(),
    })?
}

/// The editor's `Planner.bundle()`.
#[tauri::command]
#[specta::specta]
pub async fn planner_bundle(
    selection: Json,
    target: Option<String>,
    credentials: BTreeMap<String, String>,
    context: Option<Json>,
) -> Result<Json, PlanError> {
    tauri::async_runtime::spawn_blocking(move || {
        planner::bundle(selection, target.as_deref(), credentials, context)
    })
    .await
    .map_err(|e| PlanError::Plan {
        message: e.to_string(),
    })?
}
