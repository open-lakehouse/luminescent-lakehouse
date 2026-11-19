//! Executors: what a run does next.
//!
//! An executor is a pure state machine. Given the node, its run and what's
//! been observed ([`Observations`]), it returns the next [`Action`]s, and the
//! reconciler applies them through the `OmnigentDriver` and records the
//! result. No LLM makes a scheduling decision.
//!
//! The reference executors, which the built-in extension registers:
//!
//! - [`task::TaskExecutor`] (`task.v1`): implementing → in_review → fixing
//!   (up to K rounds) → awaiting_merge.
//! - [`gate::GateExecutor`] (`gate.v1`): no session; asks the condition's
//!   evaluator until the gate opens.
//! - [`plan::PlanExecutor`] (`plan.v1`): a planner session, then an
//!   approval.
//!
//! Stalls are shared: a session that fails, goes missing, or goes idle
//! without what its stage needs retries the run once (a new attempt), and
//! the second time fails it and raises a `stalled` attention item.

use std::collections::BTreeMap;

use buffa::{Message, MessageName};
use chrono::{DateTime, Duration, Utc};
use factory_proto::workgraph::v1 as wg;

use crate::driver::SessionStatus;
use crate::extension::Extensions;
use crate::registry::Registry;

pub mod gate;
pub mod plan;
pub mod task;

/// The stages the reference executors set (`Run.stage`).
pub mod stage {
    pub const IMPLEMENTING: &str = "implementing";
    pub const IN_REVIEW: &str = "in_review";
    pub const FIXING: &str = "fixing";
    pub const AWAITING_MERGE: &str = "awaiting_merge";
    /// K review rounds without a pass: a person takes over.
    pub const ESCALATED: &str = "escalated";
    pub const AWAITING_GATE: &str = "awaiting_gate";
    pub const PLANNING: &str = "planning";
}

/// The step names the reference executors use (`Step.name`).
pub mod step {
    pub const IMPLEMENT: &str = "implement";
    pub const REVIEW: &str = "review";
    pub const PLAN: &str = "plan";
    pub const GATE: &str = "gate";
}

/// The roles a session is started in, as placement names them.
pub mod role {
    pub const EXECUTOR: &str = "executor";
    pub const REVIEWER: &str = "reviewer";
    pub const PLANNER: &str = "planner";
}

/// How long an idle implementing session may go without a detected PR
/// before Omnigent's sync is given up on.
pub const PR_SYNC_GRACE: Duration = Duration::minutes(10);

/// One step's session, as last seen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionView {
    pub status: SessionStatus,
    /// The turns started so far.
    pub turns: u32,
    /// Omnigent no longer has it.
    pub missing: bool,
    pub error: String,
    pub cost_usd: f64,
    /// Whether Omnigent has synced the session's PRs from the forge.
    pub prs_synced: bool,
    /// When it was first seen idle after its last turn.
    pub idle_since: Option<DateTime<Utc>>,
}

impl SessionView {
    /// Whether the session finished its first turn and `follow_ups` more.
    pub fn done(&self, follow_ups: u32) -> bool {
        self.status == SessionStatus::Idle && self.turns > follow_ups
    }

    pub fn failed(&self) -> bool {
        self.missing || self.status == SessionStatus::Failed
    }
}

/// What the reconciler has observed for a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Observations {
    pub now: DateTime<Utc>,
    /// The sessions of the run's session steps, by step index.
    pub sessions: BTreeMap<u32, SessionView>,
    /// Review verdicts on the node, oldest first.
    pub verdicts: Vec<wg::ReviewVerdict>,
    /// The node's attention items, resolved or not.
    pub attention: Vec<wg::Attention>,
}

impl Observations {
    pub fn at(now: DateTime<Utc>) -> Self {
        Self {
            now,
            sessions: BTreeMap::new(),
            verdicts: Vec::new(),
            attention: Vec::new(),
        }
    }
}

/// What an executor sees.
pub struct ExecContext<'a> {
    pub graph: &'a wg::Graph,
    /// Every run of the graph.
    pub runs: &'a [wg::Run],
    pub node: &'a wg::Node,
    pub run: &'a wg::Run,
    pub seen: &'a Observations,
    pub extensions: &'a Extensions,
    pub registry: &'a Registry,
}

impl ExecContext<'_> {
    /// The node's payload as `M`, or its default.
    pub fn payload<M: Message + MessageName + Default>(&self) -> M {
        payload(self.node)
    }
}

/// What the reconciler does next for a run.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Start a session as a new step named `step`, placed for `role` with
    /// `profile`, and sent `prompt`.
    StartSession {
        step: String,
        role: String,
        profile: String,
        prompt: String,
    },
    /// Send a message into a step's session (a follow-up turn).
    SendMessage {
        step: u32,
        text: String,
    },
    SetStep {
        step: u32,
        state: wg::StepState,
    },
    /// Record what a gate step waits on (its one `GateWait` step).
    Wait {
        observed: String,
    },
    SetStage(String),
    RaiseAttention {
        kind: wg::AttentionKind,
        summary: String,
        step: Option<u32>,
    },
    /// The run is complete; with `node_done`, so is its node.
    Complete {
        node_done: bool,
    },
    Fail {
        reason: String,
    },
    /// Fail this attempt and start the next.
    Retry {
        reason: String,
    },
}

/// A node's payload as `M`, or its default.
pub fn payload<M: Message + MessageName + Default>(node: &wg::Node) -> M {
    node.spec
        .as_option()
        .and_then(|s| s.payload.as_option())
        .filter(|a| a.type_url == M::TYPE_URL)
        .and_then(|a| a.unpack_unchecked::<M>().ok())
        .unwrap_or_default()
}

pub fn phase(node: &wg::Node) -> wg::Phase {
    node.status
        .as_option()
        .and_then(|s| s.phase.as_known())
        .unwrap_or(wg::Phase::PHASE_UNSPECIFIED)
}

/// The latest run of `node`: the highest attempt.
pub fn latest_run<'a>(runs: &'a [wg::Run], node: &str) -> Option<&'a wg::Run> {
    runs.iter()
        .filter(|r| r.node == node)
        .max_by_key(|r| r.attempt)
}

/// A run's PRs, in its artifacts' order.
pub fn pull_requests(run: &wg::Run) -> Vec<&wg::PullRequest> {
    run.artifacts
        .iter()
        .filter_map(|a| match &a.artifact {
            Some(wg::artifact::Artifact::PullRequest(pr)) => Some(pr.as_ref()),
            _ => None,
        })
        .collect()
}

/// The last step of `run` named `name`.
pub fn last_step<'a>(run: &'a wg::Run, name: &str) -> Option<&'a wg::Step> {
    run.steps.iter().rev().find(|s| s.name == name)
}

/// A session step's payload.
pub fn session_of(step: &wg::Step) -> Option<wg::OmnigentSession> {
    step.payload
        .as_option()
        .filter(|a| a.type_url == wg::OmnigentSession::TYPE_URL)
        .and_then(|a| a.unpack_unchecked().ok())
}

/// How many follow-up messages a session step has been sent.
pub fn follow_ups(step: &wg::Step) -> u32 {
    session_of(step).map_or(0, |s| s.follow_ups)
}

/// A profile name, or the registry's default for `role`.
pub fn profile_or_default(name: &str, role: &str, registry: &Registry) -> String {
    if !name.is_empty() {
        return name.to_owned();
    }
    let d = registry.defaults();
    match role {
        role::EXECUTOR => d.executor.clone(),
        role::REVIEWER => d.reviewer.clone(),
        role::PLANNER => d.planner.clone(),
        _ => String::new(),
    }
}

/// The actions for a stalled run: retry once, then fail with attention.
pub fn stall(run: &wg::Run, max_attempts: u32, reason: impl Into<String>) -> Vec<Action> {
    let reason = reason.into();
    if run.attempt < max_attempts {
        vec![Action::Retry { reason }]
    } else {
        vec![
            Action::Fail {
                reason: reason.clone(),
            },
            Action::RaiseAttention {
                kind: wg::AttentionKind::ATTENTION_KIND_STALLED,
                summary: format!("stalled after {} attempts: {reason}", run.attempt),
                step: None,
            },
        ]
    }
}

/// Whether the run has an unresolved attention item of `kind`.
pub fn has_open(seen: &Observations, run: &wg::Run, kind: wg::AttentionKind) -> bool {
    seen.attention
        .iter()
        .any(|a| a.run == run.r#ref && !a.resolved && a.kind.as_known() == Some(kind))
}

#[cfg(test)]
pub(crate) mod testkit {
    //! Building runs and observations for the executors' table tests.

    use super::*;
    use crate::payload::pack;

    pub fn session_step(index: u32, name: &str, follow_ups: u32) -> wg::Step {
        wg::Step {
            index,
            kind: wg::StepKind::STEP_KIND_SESSION.into(),
            state: wg::StepState::STEP_STATE_ACTIVE.into(),
            name: name.into(),
            payload: pack(&wg::OmnigentSession {
                session_id: format!("sess-{index}"),
                follow_ups,
                ..Default::default()
            })
            .into(),
            ..Default::default()
        }
    }

    pub fn pr(url: &str, state: &str) -> wg::Artifact {
        wg::Artifact {
            artifact: Some(wg::artifact::Artifact::PullRequest(Box::new(
                wg::PullRequest {
                    url: url.into(),
                    state: state.into(),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        }
    }

    pub fn run(attempt: u32, stage: &str, steps: Vec<wg::Step>, prs: &[(&str, &str)]) -> wg::Run {
        wg::Run {
            r#ref: "run".into(),
            node: "n".into(),
            attempt,
            state: wg::RunState::RUN_STATE_WORKING.into(),
            stage: stage.into(),
            steps,
            artifacts: prs.iter().map(|(u, s)| pr(u, s)).collect(),
            ..Default::default()
        }
    }

    pub fn view(status: SessionStatus, turns: u32) -> SessionView {
        SessionView {
            status,
            turns,
            prs_synced: true,
            ..Default::default()
        }
    }

    pub fn node<M: Message + MessageName>(kind: &str, payload: &M) -> wg::Node {
        wg::Node {
            r#ref: "n".into(),
            kind: kind.into(),
            spec: wg::NodeSpec {
                title: "Build it".into(),
                body: "The body.".into(),
                payload: pack(payload).into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
    }
}
