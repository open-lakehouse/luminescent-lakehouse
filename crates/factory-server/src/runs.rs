//! Run records the RPCs and the reconciler share: an execution's record,
//! new runs and their next attempts, and the places runs hold in capped
//! environments.

use std::collections::{BTreeMap, BTreeSet};

use buffa_types::google::protobuf::Timestamp;
use factory_core::executor::{self, stage};
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
    run.waiting = Default::default();
    run.updated_at = now.into();
}

/// The environments `run` holds a place in: those its sessions ran in, while
/// it's live and not waiting for a merge or a person (`awaiting_merge`,
/// `escalated`), where nothing runs for it. See docs/factory/hosts.md.
pub fn places(run: &wg::Run) -> BTreeSet<String> {
    if !is_live(run) || run.stage == stage::AWAITING_MERGE || run.stage == stage::ESCALATED {
        return BTreeSet::new();
    }
    run.steps
        .iter()
        .filter_map(executor::session_of)
        .filter_map(|s| s.spec.as_option().map(|spec| spec.environment.clone()))
        .filter(|e| !e.is_empty())
        .collect()
}

/// Whether a run may start a session in a capped environment now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Admitted,
    /// Every place is taken, or promised to the runs ahead of it.
    Waiting {
        ahead: usize,
    },
}

/// Who holds and who waits for the places of every environment, across all
/// graphs: built from the runs at the start of a pass and kept current as the
/// pass starts sessions.
#[derive(Debug, Default)]
pub struct Places {
    held: BTreeMap<String, BTreeSet<String>>,
    /// By environment: `(since, run)`, so the order is the line.
    waiting: BTreeMap<String, BTreeSet<(i64, i32, String)>>,
}

impl Places {
    pub fn from_runs<'a>(runs: impl IntoIterator<Item = &'a wg::Run>) -> Self {
        let mut out = Self::default();
        for run in runs {
            for env in places(run) {
                out.take(&run.r#ref, &env);
            }
            if is_live(run)
                && let Some(w) = run.waiting.as_option()
            {
                out.wait(&run.r#ref, &w.environment, &w.since);
            }
        }
        out
    }

    /// Whether `run` may start a session in `env`, which has `max` places.
    /// A run that holds a place anywhere never waits; any other run gets a
    /// free place only once every run that started waiting before it has
    /// one.
    pub fn admit(&self, run: &wg::Run, env: &str, max: u32) -> Admission {
        if max == 0 || !places(run).is_empty() || self.held.values().any(|h| h.contains(&run.r#ref))
        {
            return Admission::Admitted;
        }
        let held = self.held.get(env).map_or(0, BTreeSet::len);
        let free = (max as usize).saturating_sub(held);
        let mine = run
            .waiting
            .as_option()
            .filter(|w| w.environment == env)
            .map(|w| key(&run.r#ref, &w.since));
        let ahead = self.waiting.get(env).map_or(0, |line| {
            line.iter()
                .filter(|k| k.2 != run.r#ref)
                .filter(|k| mine.as_ref().is_none_or(|m| *k < m))
                .count()
        });
        if ahead < free {
            Admission::Admitted
        } else {
            Admission::Waiting { ahead }
        }
    }

    /// `run` took a place in `env`: it holds it, and stops waiting.
    pub fn take(&mut self, run: &str, env: &str) {
        for line in self.waiting.values_mut() {
            line.retain(|k| k.2 != run);
        }
        self.held.entry(env.into()).or_default().insert(run.into());
    }

    /// `run` waits for a place in `env` since `since`.
    pub fn wait(&mut self, run: &str, env: &str, since: &Timestamp) {
        for line in self.waiting.values_mut() {
            line.retain(|k| k.2 != run);
        }
        self.waiting
            .entry(env.into())
            .or_default()
            .insert(key(run, since));
    }
}

fn key(run: &str, since: &Timestamp) -> (i64, i32, String) {
    (since.seconds, since.nanos, run.into())
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

#[cfg(test)]
mod tests {
    use factory_core::payload;

    use super::*;

    fn at(seconds: i64) -> Timestamp {
        Timestamp {
            seconds,
            ..Default::default()
        }
    }

    /// A live run with one session in `env`.
    fn working(r: &str, env: &str) -> wg::Run {
        let session = wg::OmnigentSession {
            spec: wg::ResolvedSessionSpec {
                environment: env.into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        };
        wg::Run {
            r#ref: r.into(),
            state: wg::RunState::RUN_STATE_WORKING.into(),
            steps: vec![wg::Step {
                payload: payload::pack(&session).into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn queued(r: &str, wait: Option<(&str, i64)>) -> wg::Run {
        wg::Run {
            r#ref: r.into(),
            state: wg::RunState::RUN_STATE_QUEUED.into(),
            waiting: wait
                .map(|(env, since)| wg::PlaceWait {
                    environment: env.into(),
                    since: at(since).into(),
                    ..Default::default()
                })
                .into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_run_holds_a_place_while_it_works() {
        let mut run = working("a", "box");
        assert_eq!(places(&run), BTreeSet::from(["box".to_owned()]));
        run.stage = stage::AWAITING_MERGE.into();
        assert!(places(&run).is_empty(), "waiting for a merge gives it back");
        let mut run = working("a", "box");
        finish(&mut run, wg::RunState::RUN_STATE_FAILED, "boom");
        assert!(places(&run).is_empty(), "so does ending");
    }

    #[test]
    fn free_places_go_to_the_runs_that_waited_longest() {
        let runs = [
            working("a", "box"),
            queued("b", Some(("box", 200))),
            queued("c", Some(("box", 300))),
        ];
        let places = Places::from_runs(&runs);
        assert_eq!(places.admit(&runs[1], "box", 2), Admission::Admitted);
        assert_eq!(
            places.admit(&runs[2], "box", 2),
            Admission::Waiting { ahead: 1 },
            "the one place left is b's"
        );
        assert_eq!(places.admit(&runs[2], "box", 3), Admission::Admitted);
        let new = queued("d", None);
        assert_eq!(
            places.admit(&new, "box", 3),
            Admission::Waiting { ahead: 2 },
            "a newcomer is behind everyone waiting"
        );
        assert_eq!(
            places.admit(&runs[0], "box", 1),
            Admission::Admitted,
            "a run that holds a place never waits"
        );
        assert_eq!(
            places.admit(&new, "box", 0),
            Admission::Admitted,
            "no limit"
        );
    }

    #[test]
    fn taking_a_place_leaves_the_line() {
        let b = queued("b", Some(("box", 200)));
        let c = queued("c", Some(("box", 300)));
        let mut places = Places::from_runs([&b, &c]);
        places.take("b", "box");
        assert_eq!(places.admit(&c, "box", 2), Admission::Admitted);
        assert_eq!(places.admit(&c, "box", 1), Admission::Waiting { ahead: 0 });
    }
}
