//! `plan.v1`: planning, then following the plan.
//!
//! | Stage | Leaves when |
//! |---|---|
//! | `planning` | the planner session is idle: an `approval` item asks a person to review the revision |
//! | `following` | every other node is done or dropped, and everything is reported |
//!
//! While it follows, the planning run keeps its planner session and reports
//! the plan's finished task stages to it ([`super::report`]): one digest per
//! pass, only when the session is idle, each event once. The run's
//! [`wg::PlannerReports`] says what's been reported and what's in flight. A
//! report counts once the session has started the turn it was sent for, so
//! a send that's lost (a crash between recording it and sending it) goes
//! again after [`REPORT_GRACE`]. When the session is gone, a new one takes
//! over with the report. With stage reports off (the plan's
//! `stage_reports`, else the registry's), the run completes once the plan is
//! submitted. The root itself stays open either way.

use buffa_types::google::protobuf::Timestamp;
use factory_proto::workgraph::v1 as wg;

use super::report::{self, StageEvent};
use super::{
    Action, ExecContext, REPORT_GRACE, executor_state, last_step, profile_or_default, role, stage,
    stall, step,
};
use crate::extension::NodeExecutor;
use crate::payload::pack;
use crate::schema::PLAN;

pub const NAME: &str = "plan.v1";

/// The reference `plan.v1` executor.
#[derive(Debug, Clone)]
pub struct PlanExecutor {
    /// Attempts before a stall fails the run (2: retry once).
    pub max_attempts: u32,
}

impl Default for PlanExecutor {
    fn default() -> Self {
        Self { max_attempts: 2 }
    }
}

impl NodeExecutor for PlanExecutor {
    fn kind(&self) -> &str {
        PLAN
    }

    fn name(&self) -> &str {
        NAME
    }

    fn next(&self, cx: &ExecContext<'_>) -> Vec<Action> {
        let plan: wg::Plan = cx.payload();
        if cx.run.stage == stage::FOLLOWING {
            return self.following(cx, &plan);
        }
        let Some(planning) = last_step(cx.run, step::PLAN) else {
            return vec![
                Action::StartSession {
                    step: step::PLAN.into(),
                    role: role::PLANNER.into(),
                    profile: planner_profile(cx, &plan),
                    prompt: prompt(cx, &plan),
                },
                Action::SetStage(stage::PLANNING.into()),
            ];
        };
        let Some(seen) = cx.seen.sessions.get(&planning.index) else {
            return Vec::new();
        };
        if seen.failed() {
            return stall(cx.run, self.max_attempts, "the planning session failed");
        }
        if !seen.done(super::follow_ups(planning)) {
            return Vec::new();
        }
        let approval = Action::RaiseAttention {
            kind: wg::AttentionKind::ATTENTION_KIND_APPROVAL,
            summary: format!("Review the plan {}", cx.node.spec.title),
            step: Some(planning.index),
        };
        if report::enabled(&plan, cx.registry) {
            // The step stays active: its session is followed for the
            // reports.
            return vec![approval, Action::SetStage(stage::FOLLOWING.into())];
        }
        vec![
            Action::SetStep {
                step: planning.index,
                state: wg::StepState::STEP_STATE_SUCCEEDED,
            },
            approval,
            Action::Complete { node_done: false },
        ]
    }
}

impl PlanExecutor {
    fn following(&self, cx: &ExecContext<'_>, plan: &wg::Plan) -> Vec<Action> {
        let Some(planner) = last_step(cx.run, step::PLAN) else {
            return vec![Action::Fail {
                reason: "the planning run has no planner step".into(),
            }];
        };
        let done = Action::SetStep {
            step: planner.index,
            state: wg::StepState::STEP_STATE_SUCCEEDED,
        };
        if !report::enabled(plan, cx.registry) {
            return vec![done, Action::Complete { node_done: false }];
        }
        let mut state = reports(cx);
        let events = report::events(cx);
        let mut out = Vec::new();
        let session = cx.seen.sessions.get(&planner.index);
        let gone = session.is_some_and(super::SessionView::failed);

        if !state.pending.is_empty() {
            let target = cx
                .run
                .steps
                .iter()
                .find(|s| s.index == state.step && s.name == step::PLAN);
            match target {
                // Recorded, then lost before its step was: start it again
                // with what was recorded (create-or-find adopts a session
                // created in between).
                None if state.step as usize == cx.run.steps.len() => {
                    let pending = pick(&events, &state.pending);
                    return vec![Action::StartSession {
                        step: step::PLAN.into(),
                        role: role::PLANNER.into(),
                        profile: planner_profile(cx, plan),
                        prompt: report::resume_prompt(cx, &pending),
                    }];
                }
                Some(t) if t.index == planner.index => {
                    let Some(view) = session else {
                        return out;
                    };
                    if view.turns > state.turn {
                        // Delivered: its turn started.
                        let pending = std::mem::take(&mut state.pending);
                        state.reported.extend(pending);
                        out.push(Action::SetState(pack(&state)));
                    } else if gone {
                        // Never started: it goes to the next session.
                        state.pending.clear();
                    } else if view.status == crate::driver::SessionStatus::Idle
                        && cx.seen.now.timestamp() - sent_at(&state) >= REPORT_GRACE.num_seconds()
                    {
                        // Recorded but never sent: send it again.
                        let pending = pick(&events, &state.pending);
                        state.turn = view.turns;
                        state.sent_at = timestamp(cx).into();
                        return vec![
                            Action::SetState(pack(&state)),
                            Action::SendMessage {
                                step: planner.index,
                                text: report::digest(cx, &pending),
                            },
                        ];
                    } else {
                        return out;
                    }
                }
                // A step that isn't the planner's any more: report again.
                _ => state.pending.clear(),
            }
        }

        let unreported: Vec<&StageEvent> = events
            .iter()
            .filter(|e| !state.reported.contains(&e.key))
            .collect();
        if unreported.is_empty() {
            let idle = session.is_none_or(|v| {
                v.failed()
                    || (v.status == crate::driver::SessionStatus::Idle && v.turns > state.turn)
            });
            let root = cx.node.r#ref.as_str();
            let finished = cx.graph.nodes.iter().filter(|n| n.r#ref != root).all(|n| {
                matches!(
                    super::phase(n),
                    wg::Phase::PHASE_DONE | wg::Phase::PHASE_DROPPED
                )
            });
            if finished && idle {
                out.extend([done, Action::Complete { node_done: false }]);
            }
            return out;
        }
        state.pending = unreported.iter().map(|e| e.key.clone()).collect();
        state.sent_at = timestamp(cx).into();
        if gone {
            // Archived, failed, or past its budget: a new session takes
            // over, with the report.
            state.step = u32::try_from(cx.run.steps.len()).unwrap_or(u32::MAX);
            state.turn = 0;
            out.extend([
                Action::SetStep {
                    step: planner.index,
                    state: wg::StepState::STEP_STATE_FAILED,
                },
                Action::SetState(pack(&state)),
                Action::StartSession {
                    step: step::PLAN.into(),
                    role: role::PLANNER.into(),
                    profile: planner_profile(cx, plan),
                    prompt: report::resume_prompt(cx, &unreported),
                },
            ]);
            return out;
        }
        match session {
            Some(v) if v.status == crate::driver::SessionStatus::Idle && v.turns > 0 => {
                state.step = planner.index;
                state.turn = v.turns;
                out.extend([
                    Action::SetState(pack(&state)),
                    Action::SendMessage {
                        step: planner.index,
                        text: report::digest(cx, &unreported),
                    },
                ]);
                out
            }
            // Busy, or not seen yet: the next pass reports it.
            _ => out,
        }
    }
}

/// The run's reports; for a new planning run of a plan that was followed
/// before, what the earlier runs reported, so a re-plan repeats nothing.
fn reports(cx: &ExecContext<'_>) -> wg::PlannerReports {
    let state: wg::PlannerReports = executor_state(cx.run);
    if cx.run.executor_state.is_set() {
        return state;
    }
    let mut seeded = wg::PlannerReports::default();
    for r in cx
        .runs
        .iter()
        .filter(|r| r.node == cx.node.r#ref && r.r#ref != cx.run.r#ref)
    {
        let earlier: wg::PlannerReports = executor_state(r);
        for key in earlier.reported {
            if !seeded.reported.contains(&key) {
                seeded.reported.push(key);
            }
        }
    }
    seeded
}

fn pick<'a>(events: &'a [StageEvent], keys: &[String]) -> Vec<&'a StageEvent> {
    events.iter().filter(|e| keys.contains(&e.key)).collect()
}

fn sent_at(state: &wg::PlannerReports) -> i64 {
    state.sent_at.as_option().map_or(0, |t| t.seconds)
}

fn timestamp(cx: &ExecContext<'_>) -> Timestamp {
    Timestamp::from_unix_secs(cx.seen.now.timestamp())
}

fn planner_profile(cx: &ExecContext<'_>, plan: &wg::Plan) -> String {
    profile_or_default(&plan.planner_profile, role::PLANNER, cx.registry)
}

fn prompt(cx: &ExecContext<'_>, plan: &wg::Plan) -> String {
    let mut out = format!(
        "Plan this goal as a work graph under the plan {} (node {}).\n\n{}\n",
        cx.node.spec.title, cx.node.r#ref, plan.goal
    );
    if !plan.repos.is_empty() {
        let repos: Vec<&str> = plan.repos.iter().map(|r| r.slug.as_str()).collect();
        out += &format!("\nTarget repositories: {}\n", repos.join(", "));
    }
    out += "\nRead the plan, add its items and edges, and submit it when it's ready.\n";
    if report::enabled(plan, cx.registry) {
        out += "\nOnce it's approved, this session stays the plan's planner: the factory \
                reports finished task stages here, and you fold what they teach into the \
                plan.\n";
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::SessionStatus::{Failed, Idle, Running};
    use crate::executor::testkit::{node, run, session_step, view};
    use crate::executor::{Observations, SessionView};
    use crate::schema::TASK;
    use chrono::{TimeZone, Utc};

    const NOW: i64 = 10_000;

    type Sessions = Vec<(u32, SessionView)>;

    fn registry(reports: bool) -> crate::Registry {
        crate::Registry::from_yaml(&format!(
            "defaults: {{planner: plan, stage_reports: {reports}}}\nprofiles: {{plan: {{agent: p}}}}\n"
        ))
        .unwrap()
    }

    /// The actions, with prompts and texts left out.
    fn shape(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .map(|a| match a {
                Action::StartSession { step, .. } => format!("start {step}"),
                Action::SendMessage { step, .. } => format!("send {step}"),
                Action::SetStep { step, state } => format!("step {step} {state:?}"),
                Action::SetStage(s) => format!("stage {s}"),
                Action::RaiseAttention { kind, .. } => format!("attention {kind:?}"),
                Action::Complete { node_done } => format!("complete {node_done}"),
                Action::Retry { .. } => "retry".into(),
                Action::SetState(any) => {
                    let r: wg::PlannerReports = any.unpack_unchecked().unwrap();
                    format!(
                        "state reported={:?} pending={:?} step={} turn={}",
                        r.reported, r.pending, r.step, r.turn
                    )
                }
                other => format!("{other:?}"),
            })
            .collect()
    }

    struct World {
        graph: wg::Graph,
        root: wg::Node,
        runs: Vec<wg::Run>,
    }

    /// A plan with one task whose run reached review: one event,
    /// `t:implemented`.
    fn world(task_done: bool) -> World {
        let root = node(PLAN, &wg::Plan::default());
        let mut task = node(TASK, &wg::Task::default());
        task.r#ref = "task".into();
        if task_done {
            task.status = wg::NodeStatus {
                phase: wg::Phase::PHASE_DONE.into(),
                ..Default::default()
            }
            .into();
        }
        let mut t = run(
            1,
            stage::IN_REVIEW,
            vec![
                session_step(0, step::IMPLEMENT, 0),
                session_step(1, step::REVIEW, 0),
            ],
            &[("https://pr/1", "open")],
        );
        t.r#ref = "t".into();
        t.node = "task".into();
        t.executor = super::super::task::NAME.into();
        World {
            graph: wg::Graph {
                nodes: vec![root.clone(), task],
                ..Default::default()
            },
            root,
            runs: vec![t],
        }
    }

    fn following(steps: Vec<wg::Step>, state: Option<wg::PlannerReports>) -> wg::Run {
        let mut r = run(1, stage::FOLLOWING, steps, &[]);
        r.executor_state = state.map(|s| pack(&s)).into();
        r
    }

    fn pending(step: u32, turn: u32, sent_at: i64) -> wg::PlannerReports {
        wg::PlannerReports {
            pending: vec!["t:implemented".into()],
            step,
            turn,
            sent_at: Timestamp::from_unix_secs(sent_at).into(),
            ..Default::default()
        }
    }

    fn next(
        w: &World,
        reports: bool,
        r: &wg::Run,
        sessions: Vec<(u32, SessionView)>,
    ) -> Vec<String> {
        let reg = registry(reports);
        let ext = crate::Extensions::builtin();
        let mut seen = Observations::at(Utc.timestamp_opt(NOW, 0).unwrap());
        seen.sessions = sessions.into_iter().collect();
        shape(&PlanExecutor::default().next(&ExecContext {
            graph: &w.graph,
            runs: &w.runs,
            node: &w.root,
            run: r,
            seen: &seen,
            extensions: &ext,
            registry: &reg,
        }))
    }

    #[test]
    fn planning() {
        let w = world(false);
        let planning = || {
            run(
                1,
                stage::PLANNING,
                vec![session_step(0, step::PLAN, 0)],
                &[],
            )
        };
        let cases: Vec<(&str, bool, wg::Run, Sessions, Vec<&str>)> = vec![
            (
                "start",
                true,
                run(1, "", vec![], &[]),
                vec![],
                vec!["start plan", "stage planning"],
            ),
            (
                "running waits",
                true,
                planning(),
                vec![(0, view(Running, 1))],
                vec![],
            ),
            (
                "idle asks for approval and follows",
                true,
                planning(),
                vec![(0, view(Idle, 1))],
                vec!["attention ATTENTION_KIND_APPROVAL", "stage following"],
            ),
            (
                "without reports it completes",
                false,
                planning(),
                vec![(0, view(Idle, 1))],
                vec![
                    "step 0 STEP_STATE_SUCCEEDED",
                    "attention ATTENTION_KIND_APPROVAL",
                    "complete false",
                ],
            ),
            (
                "failed retries",
                true,
                planning(),
                vec![(0, view(Failed, 1))],
                vec!["retry"],
            ),
        ];
        for (name, reports, r, sessions, want) in cases {
            assert_eq!(next(&w, reports, &r, sessions), want, "{name}");
        }
    }

    #[test]
    fn following_reports() {
        let w = world(false);
        let plan = || vec![session_step(0, step::PLAN, 0)];
        let missing = SessionView {
            missing: true,
            ..Default::default()
        };
        let cases: Vec<(&str, wg::Run, Sessions, Vec<&str>)> = vec![
            (
                "a busy planner waits",
                following(plan(), None),
                vec![(0, view(Running, 1))],
                vec![],
            ),
            (
                "an idle planner gets the report",
                following(plan(), None),
                vec![(0, view(Idle, 1))],
                vec![
                    "state reported=[] pending=[\"t:implemented\"] step=0 turn=1",
                    "send 0",
                ],
            ),
            (
                "a report in flight waits",
                following(plan(), Some(pending(0, 1, NOW - 10))),
                vec![(0, view(Idle, 1))],
                vec![],
            ),
            (
                "a started turn acknowledges it",
                following(plan(), Some(pending(0, 1, NOW - 10))),
                vec![(0, view(Running, 2))],
                vec!["state reported=[\"t:implemented\"] pending=[] step=0 turn=1"],
            ),
            (
                "a lost send goes again after the grace",
                following(plan(), Some(pending(0, 1, NOW - 121))),
                vec![(0, view(Idle, 1))],
                vec![
                    "state reported=[] pending=[\"t:implemented\"] step=0 turn=1",
                    "send 0",
                ],
            ),
            (
                "reported already: nothing to send",
                following(
                    plan(),
                    Some(wg::PlannerReports {
                        reported: vec!["t:implemented".into()],
                        ..Default::default()
                    }),
                ),
                vec![(0, view(Idle, 1))],
                vec![],
            ),
            (
                "a gone planner is replaced with the report",
                following(plan(), None),
                vec![(0, missing.clone())],
                vec![
                    "step 0 STEP_STATE_FAILED",
                    "state reported=[] pending=[\"t:implemented\"] step=1 turn=0",
                    "start plan",
                ],
            ),
            (
                "a report lost with its session goes to the next",
                following(plan(), Some(pending(0, 1, NOW - 10))),
                vec![(0, missing)],
                vec![
                    "step 0 STEP_STATE_FAILED",
                    "state reported=[] pending=[\"t:implemented\"] step=1 turn=0",
                    "start plan",
                ],
            ),
            (
                "a replacement recorded but not started starts",
                following(plan(), Some(pending(1, 0, NOW - 10))),
                vec![],
                vec!["start plan"],
            ),
            (
                "the replacement acknowledges on its first turn",
                following(
                    vec![
                        session_step(0, step::PLAN, 0),
                        session_step(1, step::PLAN, 0),
                    ],
                    Some(pending(1, 0, NOW - 10)),
                ),
                vec![(1, view(Running, 1))],
                vec!["state reported=[\"t:implemented\"] pending=[] step=1 turn=0"],
            ),
        ];
        for (name, r, sessions, want) in cases {
            assert_eq!(next(&w, true, &r, sessions), want, "{name}");
        }
    }

    #[test]
    fn following_ends() {
        let done = world(true);
        let reported = || {
            following(
                vec![session_step(0, step::PLAN, 0)],
                Some(wg::PlannerReports {
                    reported: vec!["t:implemented".into()],
                    turn: 1,
                    ..Default::default()
                }),
            )
        };
        assert_eq!(
            next(&done, true, &reported(), vec![(0, view(Idle, 2))]),
            ["step 0 STEP_STATE_SUCCEEDED", "complete false"],
            "everything done and reported"
        );
        assert_eq!(
            next(&done, true, &reported(), vec![(0, view(Running, 2))]),
            Vec::<String>::new(),
            "the planner is still on the last report"
        );
        assert_eq!(
            next(
                &world(false),
                false,
                &following(vec![session_step(0, step::PLAN, 0)], None),
                vec![(0, view(Idle, 1))]
            ),
            ["step 0 STEP_STATE_SUCCEEDED", "complete false"],
            "switched off while following"
        );
    }
}
