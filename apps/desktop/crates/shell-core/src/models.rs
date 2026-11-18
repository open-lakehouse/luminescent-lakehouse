//! Where an environment's models come from: its **Models** choice (docs/ai-gateway.md).
//!
//! The agents in an environment get models from MLflow's AI Gateway, or, without
//! MLflow, from Omnigent's host calling the upstream itself. Either way the upstream is
//! one of:
//!
//! - **provider keys** (`ANTHROPIC_API_KEY`, …, as variables or in the app's
//!   environment), trestle's default;
//! - a **Databricks profile** of the CLI: the literal variables
//!   [`PROFILE_VAR`](crate::databricks::PROFILE_VAR) and
//!   [`HOST_VAR`](crate::databricks::HOST_VAR), with tokens from the app's
//!   [`crate::token_endpoint`] (no token is stored);
//! - the **Neon AI Gateway** of the environment's branch (#169; not offered yet).
//!
//! The choice is the variables plus one knob, `upstream`, on `mlflow` when the draft
//! runs it, else on `omnigent`. A knob is part of a revision, and Start runs the head:
//! so when the head doesn't have it yet, choosing commits the draft, if the draft is the
//! head apart from that knob. A draft with other changes is left for the user to commit.

use olai_stack_state::{EnvironmentId, VarScope, VarValue};
use olai_stack_topology::ModuleId;
use olai_stack_topology::manifest::EnvManifest;
use serde::{Deserialize, Serialize};

use crate::databricks::{self, HOST_VAR, PROFILE_VAR};
use crate::state::{RevisionInfo, SharedState, StateError, revision_info, validate};

/// The module whose `upstream` knob the choice sets when the draft runs it.
pub const MLFLOW: &str = "mlflow";
/// The module whose `upstream` knob the choice sets when the draft runs it and not MLflow.
pub const OMNIGENT: &str = "omnigent";
/// The knob.
pub const UPSTREAM_KNOB: &str = "upstream";

/// An environment's Models choice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ModelsChoice {
    /// The providers' own keys.
    Keys,
    /// A profile of the Databricks CLI, and its workspace.
    DatabricksProfile { profile: String, host: String },
    /// The Neon AI Gateway of the environment's branch (#169).
    NeonAiGateway,
}

impl ModelsChoice {
    /// The `upstream` knob's value for it.
    fn upstream(&self) -> &'static str {
        match self {
            ModelsChoice::Keys => "keys",
            ModelsChoice::DatabricksProfile { .. } => "databricks",
            ModelsChoice::NeonAiGateway => "neon",
        }
    }
}

/// An `upstream` knob, as a revision sets it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UpstreamKnob {
    pub module: String,
    pub value: String,
}

/// An environment's Models, as the Start dialog and its settings show them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ModelsInfo {
    /// What its variables and knobs say: a profile when it names one, else keys.
    pub choice: ModelsChoice,
    /// Whether the user chose: it names a profile, or its head sets the knob.
    pub chosen: bool,
    /// Whether its draft runs something that takes models (MLflow or Omnigent).
    pub wants_models: bool,
    /// The knob the choice sets (on the draft's module), as the head revision has it;
    /// `None` when the draft runs neither module, or there is no head.
    pub applied: Option<UpstreamKnob>,
}

/// What [`SharedState::set_models`] did.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ModelsApplied {
    /// The revision it committed, if the knob changed and the draft was otherwise
    /// the head.
    pub committed: Option<RevisionInfo>,
    /// The draft has the knob, but other changes too: commit it before the next Start
    /// picks the choice up (its variables apply at once).
    pub needs_commit: bool,
}

type Result<T> = std::result::Result<T, StateError>;

fn literal(vars: &olai_stack_state::VarSet, name: &str) -> Option<String> {
    match vars.get(name) {
        Some(VarValue::Literal(v)) if !v.trim().is_empty() => Some(v.trim().to_owned()),
        _ => None,
    }
}

/// The module of `manifest`'s selection whose knob the choice sets.
fn target_module(manifest: &EnvManifest) -> Option<&'static str> {
    let runs = |m: &str| manifest.selection.modules.iter().any(|id| id.0 == m);
    if runs(MLFLOW) {
        Some(MLFLOW)
    } else if runs(OMNIGENT) {
        Some(OMNIGENT)
    } else {
        None
    }
}

fn knob(manifest: &EnvManifest, module: &str) -> Option<String> {
    manifest
        .selection
        .knob_overrides
        .get(&ModuleId(module.to_owned()))
        .and_then(|k| k.get(UPSTREAM_KNOB))
        .cloned()
}

fn set_knob(manifest: &mut EnvManifest, module: &str, value: &str) {
    manifest
        .selection
        .knob_overrides
        .entry(ModuleId(module.to_owned()))
        .or_default()
        .insert(UPSTREAM_KNOB.to_owned(), value.to_owned());
}

impl SharedState {
    /// Environment `id`'s Models.
    pub fn models(&self, id: &str) -> Result<ModelsInfo> {
        let store = self.store();
        let env_id = EnvironmentId(id.to_owned());
        let env = store.environment(&env_id)?;
        let workspace = store.workspace_by_id(&env.workspace)?;
        let var = |name: &str| literal(&env.vars, name).or_else(|| literal(&workspace.vars, name));
        let profile = var(PROFILE_VAR).zip(var(HOST_VAR));
        let module = target_module(&env.draft.manifest);
        let head = match env.head {
            Some(n) => Some(store.revision(&env_id, n)?),
            None => None,
        };
        let applied = module.zip(head.as_ref()).and_then(|(m, head)| {
            knob(head.manifest(), m).map(|value| UpstreamKnob {
                module: m.to_owned(),
                value,
            })
        });
        let choice = match profile {
            Some((profile, host)) => ModelsChoice::DatabricksProfile { profile, host },
            None => ModelsChoice::Keys,
        };
        Ok(ModelsInfo {
            chosen: matches!(choice, ModelsChoice::DatabricksProfile { .. }) || applied.is_some(),
            choice,
            wants_models: module.is_some(),
            applied,
        })
    }

    /// Choose environment `id`'s Models: write its variables (a profile's two literals,
    /// or none) and the `upstream` knob into its draft, and commit the draft when that
    /// knob is all it changes from the head.
    pub fn set_models(&self, id: &str, choice: &ModelsChoice) -> Result<ModelsApplied> {
        if let ModelsChoice::NeonAiGateway = choice {
            return Err(StateError::Invalid {
                message: "the Neon AI Gateway isn't offered yet (#169)".into(),
            });
        }
        let store = self.store();
        let env_id = EnvironmentId(id.to_owned());
        store.modify_vars(&VarScope::Environment(env_id.clone()), |vars| {
            match choice {
                ModelsChoice::DatabricksProfile { profile, host } => {
                    vars.set(PROFILE_VAR, VarValue::Literal(profile.trim().to_owned()));
                    vars.set(
                        HOST_VAR,
                        VarValue::Literal(databricks::normalize_host(host)),
                    );
                }
                _ => {
                    // Only the literals a profile choice wrote: a secret set by hand stays.
                    for name in [PROFILE_VAR, HOST_VAR] {
                        if matches!(vars.get(name), Some(VarValue::Literal(_))) {
                            vars.remove(name);
                        }
                    }
                }
            }
            Ok::<_, StateError>(())
        })?;
        let env = store.environment(&env_id)?;
        let Some(module) = target_module(&env.draft.manifest) else {
            return Ok(ModelsApplied {
                committed: None,
                needs_commit: false,
            });
        };
        let value = choice.upstream();
        let mut draft = env.draft.clone();
        set_knob(&mut draft.manifest, module, value);
        if draft != env.draft {
            store.modify_draft(&env_id, |d| {
                set_knob(&mut d.manifest, module, value);
                Ok::<_, StateError>(())
            })?;
        }
        let Some(head) = env.head else {
            // Nothing to start yet: the first commit carries it.
            return Ok(ModelsApplied {
                committed: None,
                needs_commit: false,
            });
        };
        let head = store.revision(&env_id, head)?;
        if knob(head.manifest(), module).as_deref() == Some(value) {
            return Ok(ModelsApplied {
                committed: None,
                needs_commit: false,
            });
        }
        let mut wanted = head.manifest().clone();
        set_knob(&mut wanted, module, value);
        let clean = wanted == draft.manifest
            && head.modules() == &draft.modules
            && head.vars() == &draft.vars;
        if !clean {
            return Ok(ModelsApplied {
                committed: None,
                needs_commit: true,
            });
        }
        validate(&draft)?;
        let message = match choice {
            ModelsChoice::DatabricksProfile { profile, .. } => {
                format!("Models: Databricks profile {profile}")
            }
            _ => "Models: provider keys".to_owned(),
        };
        let revision = store.commit_draft_with(&env_id, Some(&message))?;
        Ok(ModelsApplied {
            committed: Some(revision_info(&revision)),
            needs_commit: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Json;

    struct Fixture {
        _dir: tempfile::TempDir,
        state: SharedState,
        dev: String,
    }

    /// `dev` with `modules` added to the baseline, committed as revision 1.
    fn fixture(modules: &[&str]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let state = SharedState::open(Some(dir.path())).unwrap();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let env = state.workspace(&ws.id).unwrap().environments.remove(0);
        let mut selection = env.selection.clone();
        let list = selection.0["modules"].as_array_mut().unwrap();
        for m in modules {
            list.push((*m).into());
        }
        state.set_selection(&env.id, selection).unwrap();
        state.commit(&env.id).unwrap();
        Fixture {
            _dir: dir,
            state,
            dev: env.id,
        }
    }

    fn profile() -> ModelsChoice {
        ModelsChoice::DatabricksProfile {
            profile: "work".into(),
            host: "https://dbc-1.cloud.databricks.com/".into(),
        }
    }

    fn head_knob(f: &Fixture, module: &str) -> Option<String> {
        let store = f.state.store();
        let env = store.environment(&EnvironmentId(f.dev.clone())).unwrap();
        let head = store.revision(&env.id, env.head.unwrap()).unwrap();
        knob(head.manifest(), module)
    }

    #[test]
    fn a_profile_writes_two_literals_and_commits_mlflow_s_upstream() {
        let f = fixture(&["mlflow"]);
        let before = f.state.models(&f.dev).unwrap();
        assert_eq!(before.choice, ModelsChoice::Keys);
        assert!(!before.chosen);
        assert!(before.wants_models);

        let applied = f.state.set_models(&f.dev, &profile()).unwrap();
        assert_eq!(applied.committed.as_ref().map(|r| r.number), Some(2));
        assert!(!applied.needs_commit);
        assert_eq!(head_knob(&f, MLFLOW).as_deref(), Some("databricks"));

        let env = f
            .state
            .store()
            .environment(&EnvironmentId(f.dev.clone()))
            .unwrap();
        assert_eq!(
            env.vars.get(PROFILE_VAR),
            Some(&VarValue::Literal("work".into()))
        );
        assert_eq!(
            env.vars.get(HOST_VAR),
            Some(&VarValue::Literal(
                "https://dbc-1.cloud.databricks.com".into()
            ))
        );
        let after = f.state.models(&f.dev).unwrap();
        assert!(after.chosen);
        assert_eq!(
            after.choice,
            ModelsChoice::DatabricksProfile {
                profile: "work".into(),
                host: "https://dbc-1.cloud.databricks.com".into()
            }
        );
        assert_eq!(
            after.applied,
            Some(UpstreamKnob {
                module: MLFLOW.into(),
                value: "databricks".into()
            })
        );
        // Choosing it again changes nothing.
        let again = f.state.set_models(&f.dev, &profile()).unwrap();
        assert_eq!(again.committed, None);
    }

    #[test]
    fn without_mlflow_omnigent_takes_the_upstream() {
        let f = fixture(&["mlflow"]);
        // Swap mlflow for omnigent (+ uc-mcp) in a draft, as the user would, then
        // commit it with the knob omnigent needs alone.
        let mut selection: Json = f.state.workspace_selection(&f.dev);
        let list = selection.0["modules"].as_array_mut().unwrap();
        list.retain(|m| m != "mlflow");
        list.push("omnigent".into());
        selection.0["knob_overrides"]["omnigent"]["upstream"] = "keys".into();
        f.state.set_selection(&f.dev, selection).unwrap();
        f.state.commit(&f.dev).unwrap();
        let applied = f.state.set_models(&f.dev, &profile()).unwrap();
        assert!(applied.committed.is_some());
        assert_eq!(head_knob(&f, OMNIGENT).as_deref(), Some("databricks"));
    }

    #[test]
    fn a_draft_with_other_changes_is_left_to_commit() {
        let f = fixture(&["mlflow"]);
        let mut selection: Json = f.state.workspace_selection(&f.dev);
        selection.0["modules"]
            .as_array_mut()
            .unwrap()
            .push("jaeger".into());
        f.state.set_selection(&f.dev, selection).unwrap();
        let applied = f.state.set_models(&f.dev, &profile()).unwrap();
        assert_eq!(applied.committed, None);
        assert!(applied.needs_commit);
        // The draft has the knob; the head doesn't yet.
        let env = f
            .state
            .store()
            .environment(&EnvironmentId(f.dev.clone()))
            .unwrap();
        assert_eq!(
            knob(&env.draft.manifest, MLFLOW).as_deref(),
            Some("databricks")
        );
        assert_eq!(head_knob(&f, MLFLOW), None);
    }

    #[test]
    fn keys_drop_the_profile_and_set_the_knob_explicitly() {
        let f = fixture(&["mlflow"]);
        f.state.set_models(&f.dev, &profile()).unwrap();
        let applied = f.state.set_models(&f.dev, &ModelsChoice::Keys).unwrap();
        assert!(applied.committed.is_some());
        assert_eq!(head_knob(&f, MLFLOW).as_deref(), Some("keys"));
        let env = f
            .state
            .store()
            .environment(&EnvironmentId(f.dev.clone()))
            .unwrap();
        assert!(env.vars.get(PROFILE_VAR).is_none());
        let info = f.state.models(&f.dev).unwrap();
        assert_eq!(info.choice, ModelsChoice::Keys);
        assert!(info.chosen, "keys, chosen");
    }

    #[test]
    fn the_neon_ai_gateway_is_not_offered_yet() {
        let f = fixture(&["mlflow"]);
        assert!(matches!(
            f.state.set_models(&f.dev, &ModelsChoice::NeonAiGateway),
            Err(StateError::Invalid { .. })
        ));
    }

    #[test]
    fn an_environment_without_models_only_keeps_the_variables() {
        let f = fixture(&[]);
        assert!(!f.state.models(&f.dev).unwrap().wants_models);
        let applied = f.state.set_models(&f.dev, &profile()).unwrap();
        assert_eq!(applied.committed, None);
        assert!(!applied.needs_commit);
    }

    impl SharedState {
        fn workspace_selection(&self, env: &str) -> Json {
            let store = self.store();
            let env = store.environment(&EnvironmentId(env.to_owned())).unwrap();
            Json(serde_json::to_value(&env.draft.manifest.selection).unwrap())
        }
    }
}
