//! Run records the RPCs and the reconciler share: an execution's record,
//! new runs and their next attempts.

use buffa_types::google::protobuf::Timestamp;
use factory_core::{Error, Factory, Provenance, Result};
use factory_proto::workgraph::v1 as wg;
use serde::{Deserialize, Serialize};

/// The event kind `StartExecution` records.
pub const EXECUTE: &str = "execute";
/// The event kind of an assessed amendment.
pub const AMENDMENT: &str = "amendment";

/// What `StartExecution` recorded: an approved revision and where to place
/// it. The latest one is the graph's execution.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub revision: String,
    /// The `PlacementChoice` as proto-JSON.
    #[serde(default)]
    pub choice: serde_json::Value,
}

impl Execution {
    pub fn choice(&self) -> wg::PlacementChoice {
        serde_json::from_value(self.choice.clone()).unwrap_or_default()
    }
}

/// Record that the graph under `root` executes `revision` with `choice`.
pub async fn start_execution(
    factory: &Factory,
    root: &str,
    revision: &str,
    choice: &wg::PlacementChoice,
    provenance: &Provenance,
) -> Result<()> {
    let execution = Execution {
        revision: revision.into(),
        choice: serde_json::to_value(choice)?,
    };
    factory
        .append_event(
            root,
            root,
            EXECUTE,
            serde_json::to_string(&execution)?,
            provenance,
        )
        .await
}

/// The graph's execution, if it has one.
pub async fn execution(factory: &Factory, root: &str) -> Result<Option<Execution>> {
    Ok(factory
        .events(root)
        .await?
        .into_iter()
        .rev()
        .find(|e| e.kind == EXECUTE)
        .and_then(|e| serde_json::from_str(&e.detail).ok()))
}

pub fn is_live(run: &wg::Run) -> bool {
    matches!(
        run.state.as_known(),
        Some(
            wg::RunState::RUN_STATE_QUEUED
                | wg::RunState::RUN_STATE_WORKING
                | wg::RunState::RUN_STATE_INPUT_REQUIRED
        )
    )
}

/// A new queued run of `node` by `executor`: the node's next attempt.
pub async fn queue_run(
    factory: &Factory,
    root: &str,
    node: &str,
    executor: &str,
    profile: &str,
    environment: &str,
    provenance: &Provenance,
) -> Result<wg::Run> {
    let runs = factory.runs(root).await?;
    if runs.iter().any(|r| r.node == node && is_live(r)) {
        return Err(Error::precondition(format!(
            "{node} already has a live run"
        )));
    }
    let attempt = runs
        .iter()
        .filter(|r| r.node == node)
        .map(|r| r.attempt)
        .max()
        .unwrap_or(0)
        + 1;
    let revision = factory.graph(root).await?.approved.unwrap_or_default();
    let now = Timestamp::now();
    let run = wg::Run {
        r#ref: factory_core::olai::new_ref(),
        node: node.into(),
        attempt,
        state: wg::RunState::RUN_STATE_QUEUED.into(),
        executor: executor.into(),
        revision,
        profile: profile.into(),
        environment: environment.into(),
        created_at: now.clone().into(),
        updated_at: now.into(),
        ..Default::default()
    };
    factory.put_run(&run, provenance).await?;
    Ok(run)
}

/// End a run: `state` with `error`, its active steps closed, its lease
/// released.
pub fn finish(run: &mut wg::Run, state: wg::RunState, error: &str) {
    run.state = state.into();
    if !error.is_empty() {
        error.clone_into(&mut run.error);
    }
    let step_state = match state {
        wg::RunState::RUN_STATE_COMPLETED => wg::StepState::STEP_STATE_SUCCEEDED,
        wg::RunState::RUN_STATE_CANCELED => wg::StepState::STEP_STATE_CANCELED,
        _ => wg::StepState::STEP_STATE_FAILED,
    };
    let now = Timestamp::now();
    for step in &mut run.steps {
        if matches!(
            step.state.as_known(),
            Some(wg::StepState::STEP_STATE_ACTIVE | wg::StepState::STEP_STATE_PENDING)
        ) {
            step.state = step_state.into();
            step.ended_at = now.clone().into();
        }
    }
    run.lease_holder.clear();
    run.lease_expiry = Default::default();
    run.updated_at = now.into();
}

/// Retry `run`: end it (canceled if it was live, otherwise as it is) and
/// queue the node's next attempt with the same executor and overrides.
pub async fn retry(
    factory: &Factory,
    root: &str,
    run: &wg::Run,
    reason: &str,
    provenance: &Provenance,
) -> Result<wg::Run> {
    if is_live(run) {
        let mut ended = run.clone();
        finish(&mut ended, wg::RunState::RUN_STATE_FAILED, reason);
        factory.put_run(&ended, provenance).await?;
    }
    queue_run(
        factory,
        root,
        &run.node,
        &run.executor,
        &run.profile,
        &run.environment,
        provenance,
    )
    .await
}
