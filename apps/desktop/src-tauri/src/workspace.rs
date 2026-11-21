//! Commands over the shared state store and the native planner (shell-core's
//! `state` and `planner`), and the `state-changed` event that tells the UI to
//! reload after any write, the CLI's included.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use serde::Serialize;
use shell_core::Json;
use shell_core::models::{ModelsApplied, ModelsChoice, ModelsInfo};
use shell_core::planner::{self, PlanError};
use shell_core::state::{
    EnvironmentDraft, Imported, RevisionInfo, SharedState, StateError, WATCH_INTERVAL,
    WorkspaceInfo, WorkspaceSnapshot,
};
use tauri::{AppHandle, State, WebviewWindow};
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

/// Replace the draft's selection if it is still at `expected`, the `draftVersion` the
/// editor loaded; returns the new version, a `conflict` when it was edited since.
/// Versions cross as `u32` (specta exports no `u64`; they count edits).
#[tauri::command]
#[specta::specta]
pub async fn state_set_selection(
    state: State<'_, AppState>,
    id: String,
    selection: Json,
    expected: u32,
) -> Result<u32, StateError> {
    with_store(&state, move |s| {
        let version = s.set_selection_if(&id, selection, expected.into())?;
        u32::try_from(version).map_err(|_| StateError::Store {
            message: format!("draft version {version} is out of range"),
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn state_rename_environment(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), StateError> {
    with_store(&state, move |s| s.rename_environment(&id, &name)).await
}

/// The environment's Models choice (provider keys, a Databricks profile, the Neon AI
/// Gateway).
#[tauri::command]
#[specta::specta]
pub async fn models_get(state: State<'_, AppState>, id: String) -> Result<ModelsInfo, StateError> {
    with_store(&state, move |s| s.models(&id)).await
}

/// Choose the environment's Models: its variables, and the `upstream` knob, committed
/// when the draft is the head apart from it.
#[tauri::command]
#[specta::specta]
pub async fn models_set(
    state: State<'_, AppState>,
    settings: State<'_, crate::run::SharedSettings>,
    id: String,
    choice: ModelsChoice,
) -> Result<ModelsApplied, StateError> {
    // The Neon AI Gateway is the branch's: the environment's target, or the branch an
    // instance of it is bound to.
    let on_branch = settings.get().neon_targets.contains_key(&id)
        || state
            .runner
            .as_ref()
            .ok()
            .and_then(|r| r.branch_binding(&id).ok().flatten())
            .is_some();
    with_store(&state, move |s| s.set_models(&id, &choice, on_branch)).await
}

/// Ask for a directory with the native picker (a sheet on the window); `None` when
/// cancelled. The import and export commands take its path.
#[tauri::command]
#[specta::specta]
pub async fn pick_directory(window: WebviewWindow, title: String) -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .set_parent(&window)
        .pick_folder()
        .await
        .map(|dir| dir.path().display().to_string())
}

/// Import a workspace directory (`trestle-workspace.toml`) as a new workspace, or an
/// environment directory (`env.toml`) as a new environment of `workspace`, named `name`
/// or after the directory.
#[tauri::command]
#[specta::specta]
pub async fn state_import(
    state: State<'_, AppState>,
    dir: String,
    workspace: Option<String>,
    name: Option<String>,
) -> Result<Imported, StateError> {
    with_store(&state, move |s| {
        s.import(
            std::path::Path::new(&dir),
            workspace.as_deref(),
            name.as_deref(),
        )
    })
    .await
}

/// Export the workspace's head revisions to `<parent>/<workspace>`; returns that path.
#[tauri::command]
#[specta::specta]
pub async fn state_export_workspace(
    state: State<'_, AppState>,
    id: String,
    parent: String,
) -> Result<String, StateError> {
    with_store(&state, move |s| {
        s.export_workspace(&id, std::path::Path::new(&parent))
            .map(|dir| dir.display().to_string())
    })
    .await
}

/// Export a revision to `<parent>/<environment>-r<number>`; returns that path.
#[tauri::command]
#[specta::specta]
pub async fn state_export_revision(
    state: State<'_, AppState>,
    id: String,
    number: u32,
    parent: String,
) -> Result<String, StateError> {
    with_store(&state, move |s| {
        s.export_revision(&id, number, std::path::Path::new(&parent))
            .map(|dir| dir.display().to_string())
    })
    .await
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
    message: Option<String>,
) -> Result<RevisionInfo, StateError> {
    with_store(&state, move |s| s.commit_with(&id, message.as_deref())).await
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
