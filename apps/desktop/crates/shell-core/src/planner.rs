//! The environment editor's `Planner`, native: the same `olai-stack-topology`
//! projection the in-browser wasm planner wraps (`catalog`, `plan`, `bundle` in
//! trestle's `stack-topology-wasm`), so the editor previews exactly what the runtime
//! will run, planned by the same version.
//!
//! Inputs and outputs are the editor's JSON (`Selection`, `PlanContext`,
//! `CatalogDto`, `PlanResult`, `Bundle` in `@open-lakehouse/stack-wasm`'s types); the
//! DTOs are trestle's own, serialized with `serde_json` (plain objects, as the TS
//! types declare).

use std::collections::BTreeMap;

use crate::Json;
use olai_stack_topology::projection::{bundle_result_in, catalog_dto, plan_result_in};
use olai_stack_topology::{LocalCredentials, PlanCtx, Selection, TargetId};
use serde::Serialize;

/// Why planning failed: bad input, or the planner's own error (unknown, conflicting or
/// cyclic modules, …), whose message the editor shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PlanError {
    #[error("{message}")]
    Input { message: String },
    #[error("{message}")]
    Plan { message: String },
}

type Result<T> = std::result::Result<T, PlanError>;

fn input<T: serde::de::DeserializeOwned>(what: &str, value: Json) -> Result<T> {
    serde_json::from_value(value.0).map_err(|e| PlanError::Input {
        message: format!("invalid {what}: {e}"),
    })
}

fn output(dto: &impl Serialize) -> Json {
    Json(serde_json::to_value(dto).expect("the planner's DTOs serialize"))
}

fn plan_error(e: impl std::fmt::Display) -> PlanError {
    PlanError::Plan {
        message: e.to_string(),
    }
}

/// The environment context: `context` (a JSON `PlanCtx`; omitted fields keep their
/// defaults), with `target`, if given, as its deploy target. As the wasm planner does.
fn plan_ctx(context: Option<Json>, target: Option<&str>) -> Result<PlanCtx> {
    let mut ctx: PlanCtx = match context {
        Some(value) => input("context", value)?,
        None => PlanCtx::default(),
    };
    if let Some(target) = target {
        ctx.target = TargetId::from(target);
    }
    Ok(ctx)
}

/// The selectable catalog (`CatalogDto`).
pub fn catalog() -> Json {
    output(&catalog_dto())
}

/// Plan `selection` (`PlanResult`).
pub fn plan(selection: Json, target: Option<&str>, context: Option<Json>) -> Result<Json> {
    let selection: Selection = input("selection", selection)?;
    let ctx = plan_ctx(context, target)?;
    plan_result_in(&selection, &ctx)
        .map(|dto| output(&dto))
        .map_err(plan_error)
}

/// The deployable bundle for `selection`, with `credentials` for its local
/// services (`Bundle`).
pub fn bundle(
    selection: Json,
    target: Option<&str>,
    credentials: BTreeMap<String, String>,
    context: Option<Json>,
) -> Result<Json> {
    let selection: Selection = input("selection", selection)?;
    let credentials = LocalCredentials::from_map(credentials).map_err(plan_error)?;
    let ctx = plan_ctx(context, target)?;
    bundle_result_in(&selection, ctx, &credentials)
        .map(|dto| output(&dto))
        .map_err(plan_error)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn default_selection() -> Json {
        Json(catalog().0["default_selection"].clone())
    }

    fn plan_of(selection: Json, context: Option<Value>) -> Value {
        plan(selection, None, context.map(Json)).unwrap().0
    }

    #[test]
    fn the_catalog_has_modules_and_a_default_selection() {
        let catalog = catalog().0;
        let modules = catalog["modules"].as_array().expect("modules");
        assert!(modules.len() > 5, "{} modules", modules.len());
        assert!(modules.iter().all(|m| m["id"].is_string()));
        assert!(default_selection().0["modules"].is_array());
    }

    #[test]
    fn the_default_selection_plans_into_a_graph_and_files() {
        let plan = plan_of(default_selection(), None);
        assert!(
            plan["graph"]["nodes"]
                .as_array()
                .is_some_and(|n| !n.is_empty()),
            "{plan:#}"
        );
        let files = plan["files"].as_array().expect("files");
        assert!(
            files.iter().any(|f| f["path"] == "compose.yaml"),
            "{:?}",
            files.iter().map(|f| &f["path"]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_plan_follows_the_selection() {
        let mut selection = default_selection();
        let all = plan_of(selection.clone(), None);
        let dropped = selection.0["modules"]
            .as_array_mut()
            .unwrap()
            .pop()
            .unwrap();
        let fewer = plan_of(selection, None);
        assert_ne!(all, fewer, "dropping {dropped} changes the plan");
    }

    #[test]
    fn the_context_reaches_the_plan() {
        let a = plan_of(default_selection(), Some(json!({ "env_name": "alpha" })));
        let b = plan_of(default_selection(), Some(json!({ "env_name": "beta" })));
        assert_ne!(a, b);
    }

    #[test]
    fn bad_input_and_plan_errors_are_typed() {
        assert!(matches!(
            plan(Json(json!({ "modules": 1 })), None, None),
            Err(PlanError::Input { .. })
        ));
        assert!(matches!(
            plan(Json(json!({ "modules": ["no-such-module"] })), None, None),
            Err(PlanError::Plan { .. })
        ));
    }

    #[test]
    fn bundles_carry_files() {
        let bundle = bundle(default_selection(), None, BTreeMap::new(), None)
            .unwrap()
            .0;
        assert!(
            bundle["files"].as_array().is_some_and(|f| !f.is_empty()),
            "{bundle:#}"
        );
    }
}
