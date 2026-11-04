//! The shared state the app and the `trestle` CLI both work on: trestle's
//! `olai-stack-state` store (`$TRESTLE_HOME/state.db`), seen the way the embedded
//! environment editor needs it.
//!
//! The editor edits an environment's draft selection and commits revisions; the
//! store also holds what the editor doesn't show (vars, sources, instances), which
//! these calls leave as they are. Selections cross the boundary as the editor's
//! JSON `Selection` (the same serde shape as `olai_stack_topology::plan::Selection`).
//!
//! The store's API is synchronous; callers off the UI thread (Tauri commands run it
//! on a blocking thread). [`SharedState::watch`] reports changes, the app's own
//! writes included, so the UI reloads after a CLI write within one poll interval.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::Json;
use futures::{Stream, StreamExt};
use olai_stack_state::{
    Draft, ENV_FILE, EnvironmentId, Error as StoreError, Store, WorkspaceId, read_env_dir,
};
use olai_stack_topology::manifest::EnvManifest;
use olai_stack_topology::{
    Catalog, PlanCtx, Selection, WORKSPACE_FILE, baseline_catalog, baseline_selection,
};
use serde::Serialize;

/// How often the watcher checks the store for changes from other processes.
pub const WATCH_INTERVAL: Duration = Duration::from_millis(300);

/// A workspace in the picker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub mesh: bool,
    pub environments: u32,
}

/// A workspace with its environments' drafts, what the editor loads.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub name: String,
    pub mesh: bool,
    /// By name, as `trestle ws show` lists them.
    pub environments: Vec<EnvironmentDraft>,
}

/// An environment and its draft selection.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDraft {
    pub id: String,
    pub name: String,
    /// The newest revision, if any was committed.
    pub head: Option<u32>,
    /// The draft's `Selection`, as JSON.
    pub selection: Json,
}

/// A committed revision.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RevisionInfo {
    pub number: u32,
    /// The store's digest (of manifest, module manifests and vars).
    pub digest: String,
    /// The revision's `Selection`, as JSON.
    pub selection: Json,
    pub manifest_toml: String,
}

/// What [`SharedState::import`] made of a directory.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Imported {
    /// A workspace directory (`trestle-workspace.toml`): a new workspace, each member
    /// an environment at revision 1.
    Workspace { workspace: WorkspaceInfo },
    /// An environment directory (`env.toml`): a new environment of the workspace, at
    /// revision 1.
    Environment {
        workspace_id: String,
        environment: EnvironmentDraft,
    },
}

/// Why a state call failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StateError {
    #[error("{message}")]
    NotFound { message: String },
    #[error("{message}")]
    Duplicate { message: String },
    /// A name, a selection or a draft that doesn't plan.
    #[error("{message}")]
    Invalid { message: String },
    /// Committing a draft that equals its head revision.
    #[error("nothing to commit: the draft is the same as revision {head}")]
    NoChanges { head: u32 },
    #[error("{message}")]
    Store { message: String },
}

impl From<StoreError> for StateError {
    fn from(e: StoreError) -> Self {
        let message = e.to_string();
        match e {
            StoreError::NotFound { .. } => StateError::NotFound { message },
            StoreError::Duplicate { .. } => StateError::Duplicate { message },
            StoreError::InvalidName(_) | StoreError::Manifest(_) | StoreError::Export(_) => {
                StateError::Invalid { message }
            }
            _ => StateError::Store { message },
        }
    }
}

type Result<T> = std::result::Result<T, StateError>;

fn invalid(message: impl Into<String>) -> StateError {
    StateError::Invalid {
        message: message.into(),
    }
}

fn selection_from(value: Json) -> Result<Selection> {
    serde_json::from_value(value.0).map_err(|e| invalid(format!("not a selection: {e}")))
}

fn selection_json(selection: &Selection) -> Json {
    Json(serde_json::to_value(selection).expect("a Selection serializes"))
}

/// The baseline catalog plus the draft's own module manifests, as the CLI plans.
fn catalog_for(draft: &Draft) -> Result<Catalog> {
    let mut catalog = baseline_catalog();
    for (path, text) in &draft.modules {
        let custom = Catalog::from_manifests([text.as_str()])
            .map_err(|e| invalid(format!("module manifest `{path}`: {e}")))?;
        catalog = catalog.merge(custom);
    }
    Ok(catalog)
}

/// That `draft` plans (what `trestle env create` and `env commit` check).
fn validate(draft: &Draft) -> Result<()> {
    draft
        .manifest
        .plan(&catalog_for(draft)?)
        .map(drop)
        .map_err(|e| invalid(e.to_string()))
}

/// The shared state store.
pub struct SharedState {
    store: Arc<Store>,
    path: PathBuf,
}

impl SharedState {
    /// The store the `trestle` CLI uses (`$TRESTLE_HOME`, else the platform data dir),
    /// or the one under `home` (tests, a separate demo home).
    pub fn open(home: Option<&Path>) -> Result<Self> {
        let store = match home {
            Some(home) => {
                std::fs::create_dir_all(home).map_err(|e| StateError::Store {
                    message: format!("creating {}: {e}", home.display()),
                })?;
                Store::open(&home.join(olai_stack_state::DB_FILE))?
            }
            None => Store::open_default()?,
        };
        let path = store.path().to_owned();
        Ok(SharedState {
            store: Arc::new(store),
            path,
        })
    }

    /// Where the database is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `TRESTLE_HOME`: the directory the database is in, where instances render too.
    pub fn home(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    /// The store itself, for the runtime (`instances`).
    pub fn store(&self) -> Arc<Store> {
        Arc::clone(&self.store)
    }

    pub fn workspaces(&self) -> Result<Vec<WorkspaceInfo>> {
        self.store
            .workspaces()?
            .into_iter()
            .map(|ws| {
                let environments = self.store.environments(&ws.id)?.len() as u32;
                Ok(WorkspaceInfo {
                    id: ws.id.0,
                    name: ws.name,
                    mesh: ws.mesh,
                    environments,
                })
            })
            .collect()
    }

    /// A new workspace, with one environment `first_env` on the baseline selection
    /// (an empty workspace is nothing the editor can show).
    pub fn create_workspace(
        &self,
        name: &str,
        mesh: bool,
        first_env: &str,
    ) -> Result<WorkspaceInfo> {
        let ws = self.store.create_workspace(name, mesh)?;
        self.create_environment(&ws.id.0, first_env, None)?;
        Ok(WorkspaceInfo {
            id: ws.id.0,
            name: ws.name,
            mesh: ws.mesh,
            environments: 1,
        })
    }

    pub fn workspace(&self, id: &str) -> Result<WorkspaceSnapshot> {
        let ws = self.store.workspace_by_id(&WorkspaceId(id.to_owned()))?;
        let mut environments: Vec<EnvironmentDraft> = self
            .store
            .environments(&ws.id)?
            .into_iter()
            .map(|env| EnvironmentDraft {
                id: env.id.0,
                name: env.name,
                head: env.head,
                selection: selection_json(&env.draft.manifest.selection),
            })
            .collect();
        environments.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(WorkspaceSnapshot {
            id: ws.id.0,
            name: ws.name,
            mesh: ws.mesh,
            environments,
        })
    }

    /// A new environment in `workspace`, its draft on `selection` (the baseline when
    /// `None`), as `trestle env create` makes one. Refused if it doesn't plan.
    pub fn create_environment(
        &self,
        workspace: &str,
        name: &str,
        selection: Option<Json>,
    ) -> Result<EnvironmentDraft> {
        let selection = match selection {
            Some(value) => selection_from(value)?,
            None => baseline_selection(),
        };
        let draft = Draft::new(EnvManifest::new(selection, PlanCtx::default()));
        validate(&draft)?;
        let env = self
            .store
            .create_environment(&WorkspaceId(workspace.to_owned()), name, draft)?;
        Ok(EnvironmentDraft {
            id: env.id.0,
            name: env.name,
            head: env.head,
            selection: selection_json(&env.draft.manifest.selection),
        })
    }

    pub fn delete_environment(&self, id: &str) -> Result<()> {
        Ok(self
            .store
            .delete_environment(&EnvironmentId(id.to_owned()))?)
    }

    /// Replace the draft's selection, keeping its context, module manifests and vars.
    /// Not validated: the editor saves as you edit, and may pass through selections
    /// that don't plan yet; a commit checks.
    pub fn set_selection(&self, id: &str, selection: Json) -> Result<()> {
        let selection = selection_from(selection)?;
        self.store
            .modify_draft(&EnvironmentId(id.to_owned()), |draft| {
                draft.manifest.selection = selection;
                Ok::<_, StoreError>(())
            })?;
        Ok(())
    }

    /// The environment's revisions, newest first.
    pub fn revisions(&self, id: &str) -> Result<Vec<RevisionInfo>> {
        let mut revisions: Vec<RevisionInfo> = self
            .store
            .revisions(&EnvironmentId(id.to_owned()))?
            .iter()
            .map(revision_info)
            .collect();
        revisions.reverse();
        Ok(revisions)
    }

    /// One revision, `None` if the environment has no such number.
    pub fn revision(&self, id: &str, number: u32) -> Result<Option<RevisionInfo>> {
        match self.store.revision(&EnvironmentId(id.to_owned()), number) {
            Ok(revision) => Ok(Some(revision_info(&revision))),
            Err(StoreError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Commit the draft as the next revision. Refused if it doesn't plan, or if it
    /// is the same as the head revision (as `trestle env commit` refuses).
    pub fn commit(&self, id: &str) -> Result<RevisionInfo> {
        let env_id = EnvironmentId(id.to_owned());
        let env = self.store.environment(&env_id)?;
        validate(&env.draft)?;
        if let Some(head) = env.head {
            let revision = self.store.revision(&env_id, head)?;
            if revision.manifest() == &env.draft.manifest
                && revision.modules() == &env.draft.modules
                && revision.vars() == &env.draft.vars
            {
                return Err(StateError::NoChanges { head });
            }
        }
        Ok(revision_info(&self.store.commit_draft(&env_id)?))
    }

    /// Import directory `dir`, as `trestle ws import` / `env import` read it:
    ///
    /// - with a `trestle-workspace.toml`, as a new workspace (slots and `secrets.env`
    ///   aren't imported: the store leases slots and holds secret references only);
    /// - with an `env.toml`, as a new environment of `workspace`, named `name` or else
    ///   after the directory. It must plan, and is committed as revision 1.
    ///
    /// All or nothing: a failed import leaves nothing behind.
    pub fn import(
        &self,
        dir: &Path,
        workspace: Option<&str>,
        name: Option<&str>,
    ) -> Result<Imported> {
        if dir.join(WORKSPACE_FILE).is_file() {
            let ws = self.store.import_workspace(dir)?;
            let environments = self.store.environments(&ws.id)?.len() as u32;
            return Ok(Imported::Workspace {
                workspace: WorkspaceInfo {
                    id: ws.id.0,
                    name: ws.name,
                    mesh: ws.mesh,
                    environments,
                },
            });
        }
        if !dir.join(ENV_FILE).is_file() {
            return Err(invalid(format!(
                "{} holds neither a {WORKSPACE_FILE} nor an {ENV_FILE}",
                dir.display()
            )));
        }
        let workspace =
            workspace.ok_or_else(|| invalid("an environment is imported into a workspace"))?;
        let name = match name {
            Some(name) => name.to_owned(),
            None => dir
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| invalid(format!("{} has no name to use", dir.display())))?
                .to_owned(),
        };
        let draft = read_env_dir(dir)?;
        validate(&draft)?;
        let env =
            self.store
                .create_environment(&WorkspaceId(workspace.to_owned()), &name, draft)?;
        if let Err(e) = self.store.commit_draft(&env.id) {
            let _ = self.store.delete_environment(&env.id);
            return Err(e.into());
        }
        let env = self.store.environment(&env.id)?;
        Ok(Imported::Environment {
            workspace_id: workspace.to_owned(),
            environment: EnvironmentDraft {
                id: env.id.0,
                name: env.name,
                head: env.head,
                selection: selection_json(&env.draft.manifest.selection),
            },
        })
    }

    /// Export the workspace's head revisions as a workspace directory, `<parent>/<name>`,
    /// as `trestle ws export` writes one. Returns the directory. Refused if it exists, or
    /// if an environment has no revision.
    pub fn export_workspace(&self, id: &str, parent: &Path) -> Result<PathBuf> {
        let ws_id = WorkspaceId(id.to_owned());
        let ws = self.store.workspace_by_id(&ws_id)?;
        let dir = fresh_dir(parent, &ws.name)?;
        self.store.export_workspace(&ws_id, &dir).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&dir);
        })?;
        Ok(dir)
    }

    /// Export revision `number` of environment `id` as an environment directory,
    /// `<parent>/<env>-r<number>`. Returns the directory. Refused if it exists.
    pub fn export_revision(&self, id: &str, number: u32, parent: &Path) -> Result<PathBuf> {
        let env_id = EnvironmentId(id.to_owned());
        let env = self.store.environment(&env_id)?;
        // Before creating the directory: a missing revision leaves nothing behind.
        self.store.revision(&env_id, number)?;
        let dir = fresh_dir(parent, &format!("{}-r{number}", env.name))?;
        self.store
            .export_revision(&env_id, number, &dir)
            .inspect_err(|_| {
                let _ = std::fs::remove_dir_all(&dir);
            })?;
        Ok(dir)
    }

    /// One item per change to the store (any process's, ours included), checked every
    /// `interval`. Ends if the store can't be read.
    pub fn watch(&self, interval: Duration) -> Result<impl Stream<Item = ()> + Send + 'static> {
        let watcher = self.store.watcher()?;
        Ok(watcher
            .changes(interval)
            .take_while(|r| futures::future::ready(r.is_ok()))
            .map(drop))
    }
}

/// `<parent>/<name>`, which mustn't exist yet: an export never writes over files.
fn fresh_dir(parent: &Path, name: &str) -> Result<PathBuf> {
    if !parent.is_dir() {
        return Err(invalid(format!("{} is not a directory", parent.display())));
    }
    let dir = parent.join(name);
    if dir.exists() {
        return Err(StateError::Duplicate {
            message: format!("{} already exists", dir.display()),
        });
    }
    std::fs::create_dir(&dir).map_err(|e| StateError::Store {
        message: format!("creating {}: {e}", dir.display()),
    })?;
    Ok(dir)
}

fn revision_info(revision: &olai_stack_state::Revision) -> RevisionInfo {
    RevisionInfo {
        number: revision.number(),
        digest: revision.digest().to_owned(),
        selection: selection_json(&revision.manifest().selection),
        manifest_toml: revision.manifest_toml().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn temp_state() -> (tempfile::TempDir, SharedState) {
        let dir = tempfile::tempdir().unwrap();
        let state = SharedState::open(Some(dir.path())).unwrap();
        (dir, state)
    }

    fn modules(selection: &Value) -> Vec<String> {
        selection["modules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn a_new_workspace_has_a_baseline_environment() {
        let (_dir, state) = temp_state();
        assert!(state.workspaces().unwrap().is_empty());
        let ws = state.create_workspace("local", false, "dev").unwrap();
        assert_eq!(state.workspaces().unwrap(), vec![ws.clone()]);

        let snapshot = state.workspace(&ws.id).unwrap();
        assert_eq!(snapshot.name, "local");
        assert_eq!(snapshot.environments.len(), 1);
        let dev = &snapshot.environments[0];
        assert_eq!((dev.name.as_str(), dev.head), ("dev", None));
        assert_eq!(
            dev.selection,
            selection_json(&baseline_selection()),
            "the draft starts on the baseline, like `trestle env create`"
        );
    }

    #[test]
    fn duplicates_and_bad_names_are_typed_errors() {
        let (_dir, state) = temp_state();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        assert!(matches!(
            state.create_environment(&ws.id, "dev", None),
            Err(StateError::Duplicate { .. })
        ));
        assert!(matches!(
            state.create_environment(&ws.id, "not a name!", None),
            Err(StateError::Invalid { .. })
        ));
        assert!(matches!(
            state.workspace("no-such-id"),
            Err(StateError::NotFound { .. })
        ));
    }

    #[test]
    fn selection_edits_commit_as_revisions() {
        let (_dir, state) = temp_state();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let dev = state.workspace(&ws.id).unwrap().environments.remove(0);

        let first = state.commit(&dev.id).unwrap();
        assert_eq!(first.number, 1);
        assert!(
            first.manifest_toml.contains("[selection]"),
            "{}",
            first.manifest_toml
        );
        assert_eq!(
            state.commit(&dev.id),
            Err(StateError::NoChanges { head: 1 }),
            "an unchanged draft doesn't commit"
        );

        // Drop a module the baseline selects, as the editor would.
        let mut selection = dev.selection.0.clone();
        let dropped = modules(&selection).pop().unwrap();
        selection["modules"]
            .as_array_mut()
            .unwrap()
            .retain(|m| m.as_str() != Some(&dropped));
        state
            .set_selection(&dev.id, Json(selection.clone()))
            .unwrap();
        assert_eq!(
            state.workspace(&ws.id).unwrap().environments[0].selection.0,
            selection
        );

        let second = state.commit(&dev.id).unwrap();
        assert_eq!(second.number, 2);
        assert!(!modules(&second.selection.0).contains(&dropped));
        assert_eq!(
            state.workspace(&ws.id).unwrap().environments[0].head,
            Some(2)
        );

        let numbers: Vec<u32> = state
            .revisions(&dev.id)
            .unwrap()
            .iter()
            .map(|r| r.number)
            .collect();
        assert_eq!(numbers, [2, 1], "newest first");
        assert_eq!(state.revision(&dev.id, 1).unwrap(), Some(first));
        assert_eq!(state.revision(&dev.id, 9).unwrap(), None);
    }

    #[test]
    fn a_draft_that_does_not_plan_does_not_commit() {
        let (_dir, state) = temp_state();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let dev = state.workspace(&ws.id).unwrap().environments.remove(0);
        let mut selection = dev.selection.0.clone();
        selection["modules"] = serde_json::json!(["no-such-module"]);
        state.set_selection(&dev.id, Json(selection)).unwrap();
        assert!(matches!(
            state.commit(&dev.id),
            Err(StateError::Invalid { .. })
        ));
        assert!(matches!(
            state.set_selection(&dev.id, Json(serde_json::json!({ "modules": 7 }))),
            Err(StateError::Invalid { .. })
        ));
    }

    #[test]
    fn deleting_an_environment() {
        let (_dir, state) = temp_state();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let staging = state.create_environment(&ws.id, "staging", None).unwrap();
        state.delete_environment(&staging.id).unwrap();
        let names: Vec<String> = state
            .workspace(&ws.id)
            .unwrap()
            .environments
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["dev"]);
    }

    #[tokio::test]
    async fn another_process_writing_is_seen() {
        let (dir, state) = temp_state();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let mut changes = Box::pin(state.watch(Duration::from_millis(20)).unwrap());

        // A second connection to the same file, as the CLI would be.
        let cli = SharedState::open(Some(dir.path())).unwrap();
        cli.create_environment(&ws.id, "staging", None).unwrap();

        tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .expect("a change within 5s")
            .expect("the stream is still open");
        let names: Vec<String> = state
            .workspace(&ws.id)
            .unwrap()
            .environments
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["dev", "staging"]);
    }

    #[test]
    fn a_workspace_round_trips_through_its_directory() {
        let (_dir, state) = temp_state();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let dev = state.workspace(&ws.id).unwrap().environments.remove(0);
        state.commit(&dev.id).unwrap();

        let out = tempfile::tempdir().unwrap();
        let written = state.export_workspace(&ws.id, out.path()).unwrap();
        assert_eq!(written, out.path().join("local"));
        assert!(written.join(WORKSPACE_FILE).is_file());
        assert!(written.join("dev").join(ENV_FILE).is_file());
        assert!(
            matches!(
                state.export_workspace(&ws.id, out.path()),
                Err(StateError::Duplicate { .. })
            ),
            "an export never writes over a directory"
        );

        // The same name is taken in this store: import into another one.
        let (_other_dir, other) = temp_state();
        let Imported::Workspace { workspace } = other.import(&written, None, None).unwrap() else {
            panic!("a workspace directory imports as a workspace");
        };
        assert_eq!(
            (workspace.name.as_str(), workspace.environments),
            ("local", 1)
        );
        let imported = other
            .workspace(&workspace.id)
            .unwrap()
            .environments
            .remove(0);
        assert_eq!(imported.head, Some(1));
        assert_eq!(imported.selection, dev.selection);

        assert!(matches!(
            state.import(&written, None, None),
            Err(StateError::Duplicate { .. })
        ));
        assert_eq!(state.workspaces().unwrap().len(), 1, "nothing left behind");
    }

    #[test]
    fn an_exported_revision_imports_as_an_environment() {
        let (_dir, state) = temp_state();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let dev = state.workspace(&ws.id).unwrap().environments.remove(0);
        assert!(matches!(
            state.export_revision(&dev.id, 1, Path::new("/")),
            Err(StateError::NotFound { .. })
        ));
        state.commit(&dev.id).unwrap();

        let out = tempfile::tempdir().unwrap();
        let written = state.export_revision(&dev.id, 1, out.path()).unwrap();
        assert_eq!(written, out.path().join("dev-r1"));

        let Imported::Environment { environment, .. } =
            state.import(&written, Some(&ws.id), Some("copy")).unwrap()
        else {
            panic!("an environment directory imports as an environment");
        };
        assert_eq!(
            (environment.name.as_str(), environment.head),
            ("copy", Some(1))
        );
        assert_eq!(environment.selection, dev.selection);

        // Named after the directory without a name.
        let Imported::Environment { environment, .. } =
            state.import(&written, Some(&ws.id), None).unwrap()
        else {
            panic!("an environment");
        };
        assert_eq!(environment.name, "dev-r1");
        assert!(matches!(
            state.import(&written, Some(&ws.id), None),
            Err(StateError::Duplicate { .. })
        ));
        assert!(matches!(
            state.import(&written, None, None),
            Err(StateError::Invalid { .. })
        ));
    }

    #[test]
    fn a_directory_without_a_manifest_is_refused() {
        let (_dir, state) = temp_state();
        let empty = tempfile::tempdir().unwrap();
        assert!(matches!(
            state.import(empty.path(), None, None),
            Err(StateError::Invalid { .. })
        ));
        assert!(state.workspaces().unwrap().is_empty());
    }
}
