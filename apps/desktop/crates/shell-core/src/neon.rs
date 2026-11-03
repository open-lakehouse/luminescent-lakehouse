//! Neon branches as the current environment (a thin slice of #20): a branch's
//! deployed functions, bound by module id at their invocation URLs.
//!
//! The project is the workspace's (`trestle ws set-neon <ws> --project <id>`), else
//! [`PROJECT_VAR`], else [`DEFAULT_PROJECT`]. Branches and functions come from the
//! Neon CLI through olai-stack-runtime's [`NeonControl`], so the app sees what
//! `neon branches list` / `neon functions list` show, signed in as the CLI is.

use std::sync::Arc;

use olai_stack_runtime::neon::{NEON_SOURCE, NeonControl, NeonError};
use olai_stack_state::{Store, WorkspaceId};
use serde::Serialize;

use crate::instances::{CurrentEnvironment, RunError, ServiceRoot};

/// The project the Neon card lists when the workspace names none: the one
/// `neon.ts` deploys to.
pub const DEFAULT_PROJECT: &str = "cool-poetry-28343550";

/// Overrides [`DEFAULT_PROJECT`] (not a workspace's own project).
pub const PROJECT_VAR: &str = "LUMINESCENT_NEON_PROJECT";

/// The key `trestle ws set-neon` stores the project under.
const PROJECT_KEY: &str = "project_id";

/// A function's slug (`neon.ts`) → the module id the views call. Other slugs keep
/// their name.
const SLUG_MODULES: &[(&str, &str)] = &[
    ("unitycatalog", "unity-catalog"),
    ("headwaters", "headwaters"),
];

/// The module id a function deployed as `slug` serves.
pub fn module_of(slug: &str) -> &str {
    SLUG_MODULES
        .iter()
        .find(|(s, _)| *s == slug)
        .map_or(slug, |(_, module)| module)
}

/// Where the project id came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ProjectSource {
    /// `trestle ws set-neon`.
    Workspace,
    /// [`PROJECT_VAR`].
    Env,
    /// [`DEFAULT_PROJECT`].
    Default,
}

/// A branch of the project, as the Run view lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NeonBranch {
    pub id: String,
    pub name: String,
    /// The project's default branch (production).
    pub default: bool,
}

/// The workspace's Neon project and its branches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NeonBranches {
    pub project_id: String,
    pub source: ProjectSource,
    pub branches: Vec<NeonBranch>,
}

impl From<NeonError> for RunError {
    fn from(e: NeonError) -> Self {
        RunError::Neon {
            message: e.to_string(),
        }
    }
}

/// Lists a workspace's Neon branches and binds one's functions.
pub struct NeonEnvironments {
    store: Arc<Store>,
    neon: Arc<dyn NeonControl>,
    /// [`PROJECT_VAR`], read once.
    env_project: Option<String>,
}

impl NeonEnvironments {
    pub fn new(store: Arc<Store>, neon: Arc<dyn NeonControl>, env_project: Option<String>) -> Self {
        NeonEnvironments {
            store,
            neon,
            env_project: env_project.filter(|p| !p.trim().is_empty()),
        }
    }

    /// The project the workspace's Neon card lists, and where it came from.
    pub fn project(&self, workspace_id: &str) -> Result<(String, ProjectSource), RunError> {
        let ws = self
            .store
            .workspace_by_id(&WorkspaceId(workspace_id.to_owned()))?;
        let own = ws
            .sources
            .get(NEON_SOURCE)
            .and_then(|c| c.get(PROJECT_KEY))
            .and_then(|v| v.as_str())
            .filter(|p| !p.is_empty());
        Ok(match (own, &self.env_project) {
            (Some(p), _) => (p.to_owned(), ProjectSource::Workspace),
            (None, Some(p)) => (p.clone(), ProjectSource::Env),
            (None, None) => (DEFAULT_PROJECT.to_owned(), ProjectSource::Default),
        })
    }

    /// The project's branches, the default (production) first.
    pub async fn branches(&self, workspace_id: &str) -> Result<NeonBranches, RunError> {
        let (project_id, source) = self.project(workspace_id)?;
        let mut branches: Vec<NeonBranch> = self
            .neon
            .branches(&project_id)
            .await?
            .into_iter()
            .map(|b| NeonBranch {
                id: b.id,
                name: b.name,
                default: b.default,
            })
            .collect();
        branches.sort_by(|a, b| b.default.cmp(&a.default).then_with(|| a.name.cmp(&b.name)));
        Ok(NeonBranches {
            project_id,
            source,
            branches,
        })
    }

    /// The branch `name_or_id` as the current environment: its functions' roots, by
    /// module. A branch without functions is refused (nothing would be bound).
    pub async fn bind(
        &self,
        workspace_id: &str,
        name_or_id: &str,
    ) -> Result<(CurrentEnvironment, Vec<ServiceRoot>), RunError> {
        let (project_id, _) = self.project(workspace_id)?;
        let ws = self
            .store
            .workspace_by_id(&WorkspaceId(workspace_id.to_owned()))?;
        let branch =
            olai_stack_runtime::neon::find_branch(self.neon.as_ref(), &project_id, name_or_id)
                .await?;
        let mut roots: Vec<ServiceRoot> = self
            .neon
            .functions(&project_id, &branch.id)
            .await?
            .into_iter()
            .map(|f| ServiceRoot {
                module_id: module_of(&f.slug).to_owned(),
                url: f.invocation_url,
            })
            .collect();
        if roots.is_empty() {
            return Err(RunError::Neon {
                message: format!(
                    "branch `{}` has no functions deployed (`just deploy {}`)",
                    branch.name, branch.name
                ),
            });
        }
        roots.sort_by(|a, b| a.module_id.cmp(&b.module_id));
        let current = CurrentEnvironment::Neon {
            project_id,
            branch_id: branch.id,
            branch: branch.name,
            workspace_id: ws.id.0,
            workspace: ws.name,
        };
        Ok((current, roots))
    }
}

#[cfg(test)]
mod tests {
    use olai_stack_runtime::neon::FakeNeon;
    use olai_stack_state::Store;

    use super::*;

    fn store_with_workspace() -> (tempfile::TempDir, Arc<Store>, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join(olai_stack_state::DB_FILE)).unwrap());
        let ws = store.create_workspace("local", false).unwrap();
        (dir, store, ws.id.0)
    }

    #[test]
    fn slugs_map_to_the_views_module_ids() {
        assert_eq!(module_of("unitycatalog"), "unity-catalog");
        assert_eq!(module_of("headwaters"), "headwaters");
        assert_eq!(module_of("opensharing"), "opensharing");
    }

    #[tokio::test]
    async fn the_project_is_the_workspaces_then_the_env_then_the_default() {
        let (_dir, store, ws) = store_with_workspace();
        let fake = Arc::new(FakeNeon::new("p-env"));
        let neon = NeonEnvironments::new(store.clone(), fake.clone(), None);
        assert_eq!(
            neon.project(&ws).unwrap(),
            (DEFAULT_PROJECT.to_owned(), ProjectSource::Default)
        );
        let neon = NeonEnvironments::new(store.clone(), fake.clone(), Some("p-env".into()));
        assert_eq!(
            neon.project(&ws).unwrap(),
            ("p-env".to_owned(), ProjectSource::Env)
        );
        store
            .set_workspace_source(
                &WorkspaceId(ws.clone()),
                NEON_SOURCE,
                Some(serde_json::json!({ "project_id": "p-ws" })),
            )
            .unwrap();
        assert_eq!(
            neon.project(&ws).unwrap(),
            ("p-ws".to_owned(), ProjectSource::Workspace)
        );
    }

    #[tokio::test]
    async fn binds_the_functions_of_a_branch_by_module() {
        let (_dir, store, ws) = store_with_workspace();
        let fake = FakeNeon::new("p");
        fake.deploy_function("main", "unitycatalog");
        fake.deploy_function("main", "headwaters");
        let neon = NeonEnvironments::new(store, Arc::new(fake), Some("p".into()));

        let listed = neon.branches(&ws).await.unwrap();
        assert_eq!(listed.project_id, "p");
        assert_eq!(listed.branches[0].name, "main");
        assert!(listed.branches[0].default);

        let (current, roots) = neon.bind(&ws, "main").await.unwrap();
        assert_eq!(
            current,
            CurrentEnvironment::Neon {
                project_id: "p".into(),
                branch_id: "br-main".into(),
                branch: "main".into(),
                workspace_id: ws.clone(),
                workspace: "local".into(),
            }
        );
        let ids: Vec<&str> = roots.iter().map(|r| r.module_id.as_str()).collect();
        assert_eq!(ids, ["headwaters", "unity-catalog"]);
        assert_eq!(
            roots[1].url,
            "https://br-main-unitycatalog.compute.fake.neon.tech/"
        );
    }

    #[tokio::test]
    async fn a_branch_without_functions_or_an_unknown_one_is_refused() {
        let (_dir, store, ws) = store_with_workspace();
        let neon = NeonEnvironments::new(store, Arc::new(FakeNeon::new("p")), Some("p".into()));
        let err = neon.bind(&ws, "main").await.unwrap_err();
        assert!(matches!(&err, RunError::Neon { message } if message.contains("no functions")));
        let err = neon.bind(&ws, "nope").await.unwrap_err();
        assert!(matches!(err, RunError::Neon { .. }));
    }
}
