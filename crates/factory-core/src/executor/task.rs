//! `task.v1`: implementation, review and merge.
//!
//! | Stage | Step | Leaves when |
//! |---|---|---|
//! | `implementing` | a session on the task's `profile` | it's idle with at least one PR |
//! | `in_review` | a session on `review_profile`, given the PRs | it posts a `ReviewVerdict` |
//! | `fixing` | the findings sent into the implementing session | it's idle again: back to `in_review` |
//! | `awaiting_merge` | none | every PR is merged: the node is done |
//!
//! After `max_review_rounds` reviews without a pass, the run raises a
//! `review_escalation` item and waits (`escalated`) instead of failing: a
//! person takes the PRs over, and the run completes when they merge.

use buffa_types::google::protobuf::Any;
use factory_proto::workgraph::v1 as wg;

use super::{
    Action, ExecContext, PR_SYNC_GRACE, follow_ups, last_step, profile_or_default, pull_requests,
    role, stage, stall, step,
};
use crate::extension::NodeExecutor;
use crate::schema::TASK;

/// The reference `task.v1` executor.
#[derive(Debug, Clone)]
pub struct TaskExecutor {
    /// Reviews without a pass before a person is asked.
    pub max_review_rounds: u32,
    /// Attempts before a stall fails the run (2: retry once).
    pub max_attempts: u32,
}

impl Default for TaskExecutor {
    fn default() -> Self {
        Self {
            max_review_rounds: 3,
            max_attempts: 2,
        }
    }
}

pub const NAME: &str = "task.v1";

impl NodeExecutor for TaskExecutor {
    fn kind(&self) -> &str {
        TASK
    }

    fn name(&self) -> &str {
        NAME
    }

    fn next(&self, cx: &ExecContext<'_>) -> Vec<Action> {
        let task: wg::Task = cx.payload();
        let mut actions = match cx.run.stage.as_str() {
            "" => self.implement(cx, &task),
            stage::IMPLEMENTING => self.implementing(cx, &task),
            stage::IN_REVIEW => self.in_review(cx, &task),
            stage::FIXING => self.fixing(cx, &task),
            stage::AWAITING_MERGE | stage::ESCALATED => self.awaiting_merge(cx),
            other => vec![Action::Fail {
                reason: format!("task.v1 has no stage {other:?}"),
            }],
        };
        let hooks: Vec<(&Any, &dyn crate::extension::TaskHook)> = task
            .extensions
            .iter()
            .filter_map(|e| cx.extensions.task_hook(&e.type_url).map(|h| (e, h)))
            .collect();
        for action in &mut actions {
            match action {
                Action::StartSession { role, prompt, .. } => {
                    for (e, h) in &hooks {
                        h.prompt(e, role, prompt);
                    }
                }
                Action::SendMessage { text, .. } => {
                    for (e, h) in &hooks {
                        h.prompt(e, stage::FIXING, text);
                    }
                }
                _ => {}
            }
        }
        for (e, h) in &hooks {
            h.react(e, cx, &mut actions);
        }
        actions
    }
}

impl TaskExecutor {
    fn stall(&self, cx: &ExecContext<'_>, reason: impl Into<String>) -> Vec<Action> {
        stall(cx.run, self.max_attempts, reason)
    }

    fn implement(&self, cx: &ExecContext<'_>, task: &wg::Task) -> Vec<Action> {
        vec![
            Action::StartSession {
                step: step::IMPLEMENT.into(),
                role: role::EXECUTOR.into(),
                profile: profile_or_default(&task.profile, role::EXECUTOR, cx.registry),
                prompt: implement_prompt(cx, task),
            },
            Action::SetStage(stage::IMPLEMENTING.into()),
        ]
    }

    fn review(&self, cx: &ExecContext<'_>, task: &wg::Task) -> Vec<Action> {
        vec![
            Action::StartSession {
                step: step::REVIEW.into(),
                role: role::REVIEWER.into(),
                profile: profile_or_default(&task.review_profile, role::REVIEWER, cx.registry),
                prompt: review_prompt(cx, task),
            },
            Action::SetStage(stage::IN_REVIEW.into()),
        ]
    }

    fn implementing(&self, cx: &ExecContext<'_>, task: &wg::Task) -> Vec<Action> {
        // The stage was set, but the step wasn't recorded: start it again
        // (create-or-find finds the session if it was created).
        let Some(implement) = last_step(cx.run, step::IMPLEMENT) else {
            return self.implement(cx, task);
        };
        let Some(seen) = cx.seen.sessions.get(&implement.index) else {
            return Vec::new();
        };
        if seen.failed() {
            return self.stall(cx, session_failure("implementing", seen));
        }
        if !seen.done(follow_ups(implement)) {
            return Vec::new();
        }
        if pull_requests(cx.run).is_empty() {
            let waited = seen
                .idle_since
                .is_some_and(|t| cx.seen.now - t >= PR_SYNC_GRACE);
            return if seen.prs_synced || waited {
                self.stall(cx, "the implementing session went idle without a PR")
            } else {
                Vec::new()
            };
        }
        let mut out = vec![Action::SetStep {
            step: implement.index,
            state: wg::StepState::STEP_STATE_SUCCEEDED,
        }];
        out.extend(self.review(cx, task));
        out
    }

    fn in_review(&self, cx: &ExecContext<'_>, task: &wg::Task) -> Vec<Action> {
        if all_merged(cx.run) {
            return vec![Action::Complete { node_done: true }];
        }
        let Some(review) = last_step(cx.run, step::REVIEW) else {
            return self.review(cx, task);
        };
        let verdict = cx
            .seen
            .verdicts
            .iter()
            .rev()
            .find(|v| v.run == cx.run.r#ref && v.step == review.index);
        let done_review = Action::SetStep {
            step: review.index,
            state: wg::StepState::STEP_STATE_SUCCEEDED,
        };
        match verdict {
            Some(v) if v.pass => vec![done_review, Action::SetStage(stage::AWAITING_MERGE.into())],
            Some(v) => {
                let rounds = cx
                    .run
                    .steps
                    .iter()
                    .filter(|s| s.name == step::REVIEW)
                    .count() as u32;
                if rounds >= self.max_review_rounds {
                    return vec![
                        done_review,
                        Action::RaiseAttention {
                            kind: wg::AttentionKind::ATTENTION_KIND_REVIEW_ESCALATION,
                            summary: format!(
                                "{rounds} review round(s) without a pass on {}",
                                cx.node.spec.title
                            ),
                            step: Some(review.index),
                        },
                        Action::SetStage(stage::ESCALATED.into()),
                    ];
                }
                let Some(implement) = last_step(cx.run, step::IMPLEMENT) else {
                    return self.stall(cx, "the run has no implementing step to fix in");
                };
                vec![
                    done_review,
                    Action::SendMessage {
                        step: implement.index,
                        text: fix_message(&v.findings, rounds),
                    },
                    Action::SetStep {
                        step: implement.index,
                        state: wg::StepState::STEP_STATE_ACTIVE,
                    },
                    Action::SetStage(stage::FIXING.into()),
                ]
            }
            None => {
                let Some(seen) = cx.seen.sessions.get(&review.index) else {
                    return Vec::new();
                };
                if seen.failed() {
                    self.stall(cx, session_failure("review", seen))
                } else if seen.done(follow_ups(review)) {
                    self.stall(cx, "the review session ended without a verdict")
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn fixing(&self, cx: &ExecContext<'_>, task: &wg::Task) -> Vec<Action> {
        let Some(implement) = last_step(cx.run, step::IMPLEMENT) else {
            return self.stall(cx, "the run has no implementing step to fix in");
        };
        let Some(seen) = cx.seen.sessions.get(&implement.index) else {
            return Vec::new();
        };
        if seen.failed() {
            return self.stall(cx, session_failure("implementing", seen));
        }
        if !seen.done(follow_ups(implement)) {
            return Vec::new();
        }
        let mut out = vec![Action::SetStep {
            step: implement.index,
            state: wg::StepState::STEP_STATE_SUCCEEDED,
        }];
        out.extend(self.review(cx, task));
        out
    }

    fn awaiting_merge(&self, cx: &ExecContext<'_>) -> Vec<Action> {
        let prs = pull_requests(cx.run);
        if let Some(closed) = prs.iter().find(|p| p.state == "closed") {
            return self.stall(cx, format!("{} was closed without merging", closed.url));
        }
        if all_merged(cx.run) {
            vec![Action::Complete { node_done: true }]
        } else {
            Vec::new()
        }
    }
}

fn all_merged(run: &wg::Run) -> bool {
    let prs = pull_requests(run);
    !prs.is_empty() && prs.iter().all(|p| p.state == "merged")
}

fn session_failure(what: &str, seen: &super::SessionView) -> String {
    if seen.missing {
        format!("the {what} session is gone")
    } else if seen.error.is_empty() {
        format!("the {what} session failed")
    } else {
        format!("the {what} session failed: {}", seen.error)
    }
}

fn task_prompt(cx: &ExecContext<'_>, task: &wg::Task) -> String {
    let spec = cx.node.spec.as_option().cloned().unwrap_or_default();
    let body = match task.prompt.as_option().and_then(|p| p.source.as_ref()) {
        Some(wg::prompt_ref::Source::Inline(text)) if !text.is_empty() => text.clone(),
        _ => spec.body.clone(),
    };
    let mut out = format!("# {}\n\n{body}\n", spec.title);
    if !task.acceptance.is_empty() {
        out += &format!("\n## Acceptance\n\n{}\n", task.acceptance);
    }
    if !task.touched_paths.is_empty() {
        out += &format!("\nExpected paths: {}\n", task.touched_paths.join(", "));
    }
    out
}

fn implement_prompt(cx: &ExecContext<'_>, task: &wg::Task) -> String {
    let mut out = task_prompt(cx, task);
    out += "\n## Delivery\n\n";
    if !task.repo.is_empty() {
        let base = if task.base_branch.is_empty() {
            "the default branch"
        } else {
            &task.base_branch
        };
        out += &format!("Work in {} against {base}.\n", task.repo);
    }
    if !task.change_title.is_empty() {
        out += &format!("Title the PR `{}`.\n", task.change_title);
    }
    out += &format!(
        "Open the PR(s) when the change is ready. If it's a group of PRs, report \
         them in order with `report_change` for run {}.\n",
        cx.run.r#ref
    );
    out
}

fn review_prompt(cx: &ExecContext<'_>, task: &wg::Task) -> String {
    let mut out = format!(
        "Review the change for this task.\n\n{}",
        task_prompt(cx, task)
    );
    out += "\n## PRs\n\n";
    for pr in pull_requests(cx.run) {
        out += &format!("- {}\n", pr.url);
    }
    out += &format!(
        "\nWhen you're done, post your verdict with `annotate` on node {}: a \
         ReviewVerdict with run {} and step {}, `pass`, and the findings to fix.\n",
        cx.node.r#ref,
        cx.run.r#ref,
        cx.run.steps.len()
    );
    out
}

fn fix_message(findings: &str, round: u32) -> String {
    format!(
        "Review round {round} asks for changes. Fix them on the same branch(es) and push.\n\n{findings}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::SessionStatus::{Failed, Idle, Running};
    use crate::executor::testkit::{node, run, session_step, view};
    use crate::executor::{Observations, SessionView};
    use crate::extension::{Extension, FactoryBuilder, TaskHook};
    use crate::registry::Registry;
    use chrono::{TimeZone, Utc};

    fn registry() -> Registry {
        Registry::from_yaml(
            "defaults: {executor: exec, reviewer: rev, planner: plan}\n\
             profiles: {exec: {agent: e}, rev: {agent: r}, plan: {agent: p}}\n",
        )
        .unwrap()
    }

    fn verdict(step: u32, pass: bool) -> wg::ReviewVerdict {
        wg::ReviewVerdict {
            run: "run".into(),
            step,
            pass,
            findings: "rename x".into(),
            ..Default::default()
        }
    }

    /// The actions, with prompts and texts left out: (kind, detail).
    fn shape(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .map(|a| match a {
                Action::StartSession { step, profile, .. } => format!("start {step} {profile}"),
                Action::SendMessage { step, .. } => format!("send {step}"),
                Action::SetStep { step, state } => format!("step {step} {state:?}"),
                Action::Wait { observed } => format!("wait {observed}"),
                Action::SetStage(s) => format!("stage {s}"),
                Action::RaiseAttention { kind, .. } => format!("attention {kind:?}"),
                Action::Complete { node_done } => format!("complete {node_done}"),
                Action::Fail { reason } => format!("fail {reason}"),
                Action::Retry { reason } => format!("retry {reason}"),
                Action::SetState(any) => format!("state {}", any.type_url),
            })
            .collect()
    }

    struct Case {
        name: &'static str,
        run: wg::Run,
        sessions: Vec<(u32, SessionView)>,
        verdicts: Vec<wg::ReviewVerdict>,
        want: Vec<&'static str>,
    }

    fn check(
        exec: &TaskExecutor,
        extensions: &crate::Extensions,
        task: &wg::Task,
        cases: Vec<Case>,
    ) {
        let reg = registry();
        let now = Utc.timestamp_opt(10_000, 0).unwrap();
        for c in cases {
            let n = node(TASK, task);
            let graph = wg::Graph {
                nodes: vec![n.clone()],
                ..Default::default()
            };
            let mut seen = Observations::at(now);
            seen.sessions = c.sessions.into_iter().collect();
            seen.verdicts = c.verdicts;
            let runs = [c.run.clone()];
            let cx = ExecContext {
                graph: &graph,
                runs: &runs,
                node: &n,
                run: &c.run,
                seen: &seen,
                extensions,
                registry: &reg,
            };
            assert_eq!(shape(&exec.next(&cx)), c.want, "{}", c.name);
        }
    }

    #[test]
    fn transitions() {
        let imp = |f| session_step(0, step::IMPLEMENT, f);
        let rev = |i| session_step(i, step::REVIEW, 0);
        let pr = [("u1", "open")];
        let idle_long = SessionView {
            status: Idle,
            turns: 1,
            prs_synced: false,
            idle_since: Some(Utc.timestamp_opt(10_000 - 601, 0).unwrap()),
            ..Default::default()
        };
        let idle_new = SessionView {
            idle_since: Some(Utc.timestamp_opt(10_000 - 60, 0).unwrap()),
            ..idle_long.clone()
        };
        let cases = vec![
            Case {
                name: "a new run implements",
                run: run(1, "", vec![], &[]),
                sessions: vec![],
                verdicts: vec![],
                want: vec!["start implement exec", "stage implementing"],
            },
            Case {
                name: "a lost step starts again",
                run: run(1, stage::IMPLEMENTING, vec![], &[]),
                sessions: vec![],
                verdicts: vec![],
                want: vec!["start implement exec", "stage implementing"],
            },
            Case {
                name: "not observed yet waits",
                run: run(1, stage::IMPLEMENTING, vec![imp(0)], &[]),
                sessions: vec![],
                verdicts: vec![],
                want: vec![],
            },
            Case {
                name: "launching isn't idle",
                run: run(1, stage::IMPLEMENTING, vec![imp(0)], &[]),
                sessions: vec![(0, view(Idle, 0))],
                verdicts: vec![],
                want: vec![],
            },
            Case {
                name: "running waits",
                run: run(1, stage::IMPLEMENTING, vec![imp(0)], &pr),
                sessions: vec![(0, view(Running, 1))],
                verdicts: vec![],
                want: vec![],
            },
            Case {
                name: "idle with a PR goes to review",
                run: run(1, stage::IMPLEMENTING, vec![imp(0)], &pr),
                sessions: vec![(0, view(Idle, 1))],
                verdicts: vec![],
                want: vec![
                    "step 0 STEP_STATE_SUCCEEDED",
                    "start review rev",
                    "stage in_review",
                ],
            },
            Case {
                name: "idle without a PR stalls: retry",
                run: run(1, stage::IMPLEMENTING, vec![imp(0)], &[]),
                sessions: vec![(0, view(Idle, 1))],
                verdicts: vec![],
                want: vec!["retry the implementing session went idle without a PR"],
            },
            Case {
                name: "a second stall fails with attention",
                run: run(2, stage::IMPLEMENTING, vec![imp(0)], &[]),
                sessions: vec![(0, view(Idle, 1))],
                verdicts: vec![],
                want: vec![
                    "fail the implementing session went idle without a PR",
                    "attention ATTENTION_KIND_STALLED",
                ],
            },
            Case {
                name: "unsynced PRs get a grace period",
                run: run(1, stage::IMPLEMENTING, vec![imp(0)], &[]),
                sessions: vec![(0, idle_new)],
                verdicts: vec![],
                want: vec![],
            },
            Case {
                name: "after the grace it stalls",
                run: run(1, stage::IMPLEMENTING, vec![imp(0)], &[]),
                sessions: vec![(0, idle_long)],
                verdicts: vec![],
                want: vec!["retry the implementing session went idle without a PR"],
            },
            Case {
                name: "a failed session stalls",
                run: run(1, stage::IMPLEMENTING, vec![imp(0)], &[]),
                sessions: vec![(
                    0,
                    SessionView {
                        error: "llm: overloaded".into(),
                        ..view(Failed, 1)
                    },
                )],
                verdicts: vec![],
                want: vec!["retry the implementing session failed: llm: overloaded"],
            },
            Case {
                name: "a missing session stalls",
                run: run(1, stage::IMPLEMENTING, vec![imp(0)], &[]),
                sessions: vec![(
                    0,
                    SessionView {
                        missing: true,
                        ..Default::default()
                    },
                )],
                verdicts: vec![],
                want: vec!["retry the implementing session is gone"],
            },
            Case {
                name: "a review in progress waits",
                run: run(1, stage::IN_REVIEW, vec![imp(0), rev(1)], &pr),
                sessions: vec![(1, view(Running, 1))],
                verdicts: vec![],
                want: vec![],
            },
            Case {
                name: "a pass awaits the merge",
                run: run(1, stage::IN_REVIEW, vec![imp(0), rev(1)], &pr),
                sessions: vec![(1, view(Idle, 1))],
                verdicts: vec![verdict(1, true)],
                want: vec!["step 1 STEP_STATE_SUCCEEDED", "stage awaiting_merge"],
            },
            Case {
                name: "a verdict for an old round doesn't count",
                run: run(1, stage::IN_REVIEW, vec![imp(0), rev(1), rev(2)], &pr),
                sessions: vec![(2, view(Running, 1))],
                verdicts: vec![verdict(1, true)],
                want: vec![],
            },
            Case {
                name: "changes requested: fix",
                run: run(1, stage::IN_REVIEW, vec![imp(0), rev(1)], &pr),
                sessions: vec![(1, view(Idle, 1))],
                verdicts: vec![verdict(1, false)],
                want: vec![
                    "step 1 STEP_STATE_SUCCEEDED",
                    "send 0",
                    "step 0 STEP_STATE_ACTIVE",
                    "stage fixing",
                ],
            },
            Case {
                name: "K rounds escalate",
                run: run(
                    1,
                    stage::IN_REVIEW,
                    vec![imp(2), rev(1), rev(2), rev(3)],
                    &pr,
                ),
                sessions: vec![(3, view(Idle, 1))],
                verdicts: vec![verdict(3, false)],
                want: vec![
                    "step 3 STEP_STATE_SUCCEEDED",
                    "attention ATTENTION_KIND_REVIEW_ESCALATION",
                    "stage escalated",
                ],
            },
            Case {
                name: "a review without a verdict stalls",
                run: run(1, stage::IN_REVIEW, vec![imp(0), rev(1)], &pr),
                sessions: vec![(1, view(Idle, 1))],
                verdicts: vec![],
                want: vec!["retry the review session ended without a verdict"],
            },
            Case {
                name: "merged during review completes",
                run: run(
                    1,
                    stage::IN_REVIEW,
                    vec![imp(0), rev(1)],
                    &[("u1", "merged")],
                ),
                sessions: vec![],
                verdicts: vec![],
                want: vec!["complete true"],
            },
            Case {
                name: "fixing waits for the follow-up turn",
                run: run(1, stage::FIXING, vec![imp(1), rev(1)], &pr),
                sessions: vec![(0, view(Idle, 1))],
                verdicts: vec![],
                want: vec![],
            },
            Case {
                name: "fixed: review again",
                run: run(1, stage::FIXING, vec![imp(1), rev(1)], &pr),
                sessions: vec![(0, view(Idle, 2))],
                verdicts: vec![],
                want: vec![
                    "step 0 STEP_STATE_SUCCEEDED",
                    "start review rev",
                    "stage in_review",
                ],
            },
            Case {
                name: "fixing fails: stall",
                run: run(1, stage::FIXING, vec![imp(1), rev(1)], &pr),
                sessions: vec![(0, view(Failed, 2))],
                verdicts: vec![],
                want: vec!["retry the implementing session failed"],
            },
            Case {
                name: "awaiting a merge",
                run: run(
                    1,
                    stage::AWAITING_MERGE,
                    vec![imp(0), rev(1)],
                    &[("u1", "merged"), ("u2", "open")],
                ),
                sessions: vec![],
                verdicts: vec![],
                want: vec![],
            },
            Case {
                name: "every PR merged completes",
                run: run(
                    1,
                    stage::AWAITING_MERGE,
                    vec![imp(0), rev(1)],
                    &[("u1", "merged"), ("u2", "merged")],
                ),
                sessions: vec![],
                verdicts: vec![],
                want: vec!["complete true"],
            },
            Case {
                name: "a closed PR stalls",
                run: run(
                    1,
                    stage::AWAITING_MERGE,
                    vec![imp(0), rev(1)],
                    &[("u1", "closed")],
                ),
                sessions: vec![],
                verdicts: vec![],
                want: vec!["retry u1 was closed without merging"],
            },
            Case {
                name: "escalated completes on merge",
                run: run(
                    1,
                    stage::ESCALATED,
                    vec![imp(0), rev(1)],
                    &[("u1", "merged")],
                ),
                sessions: vec![],
                verdicts: vec![],
                want: vec!["complete true"],
            },
            Case {
                name: "an unknown stage fails",
                run: run(1, "nope", vec![], &[]),
                sessions: vec![],
                verdicts: vec![],
                want: vec!["fail task.v1 has no stage \"nope\""],
            },
        ];
        let task = wg::Task {
            repo: "o/r".into(),
            ..Default::default()
        };
        check(
            &TaskExecutor::default(),
            &crate::Extensions::builtin(),
            &task,
            cases,
        );
    }

    #[test]
    fn prompts_say_what_and_how() {
        let reg = registry();
        let task = wg::Task {
            repo: "o/r".into(),
            base_branch: "main".into(),
            change_title: "feat(x): y".into(),
            acceptance: "it works".into(),
            ..Default::default()
        };
        let n = node(TASK, &task);
        let graph = wg::Graph::default();
        let seen = Observations::at(Utc::now());
        let r = run(1, "", vec![], &[]);
        let ext = crate::Extensions::builtin();
        let cx = ExecContext {
            graph: &graph,
            runs: &[],
            node: &n,
            run: &r,
            seen: &seen,
            extensions: &ext,
            registry: &reg,
        };
        let Action::StartSession { prompt, .. } = &TaskExecutor::default().next(&cx)[0] else {
            panic!()
        };
        for want in [
            "# Build it",
            "The body.",
            "it works",
            "o/r against main",
            "`feat(x): y`",
            "run run",
        ] {
            assert!(prompt.contains(want), "{want} not in {prompt}");
        }
    }

    /// A stack hook: adds to prompts and holds completion while the last
    /// layer is unmerged.
    struct Stack;
    impl TaskHook for Stack {
        fn extension_type(&self) -> &str {
            <wg::ReconcileNote as buffa::MessageName>::TYPE_URL
        }
        fn display_name(&self) -> &str {
            "Stack"
        }
        fn validate(&self, _: &Any) -> crate::Result<()> {
            Ok(())
        }
        fn prompt(&self, _: &Any, role: &str, prompt: &mut String) {
            prompt.push_str(&format!("\n[stack:{role}]"));
        }
        fn react(&self, _: &Any, cx: &ExecContext<'_>, actions: &mut Vec<Action>) {
            if cx.run.stage == stage::AWAITING_MERGE {
                actions.retain(|a| !matches!(a, Action::Complete { .. }));
                actions.push(Action::Wait {
                    observed: "the release".into(),
                });
            }
        }
    }
    struct StackExt;
    impl Extension for StackExt {
        fn name(&self) -> &'static str {
            "stack"
        }
        fn task_hooks(&self) -> Vec<Box<dyn TaskHook>> {
            vec![Box::new(Stack)]
        }
    }

    #[test]
    fn hooks_shape_prompts_and_actions() {
        let ext = FactoryBuilder::new()
            .extension(StackExt)
            .extensions()
            .unwrap();
        let task = wg::Task {
            extensions: vec![crate::payload::pack(&wg::ReconcileNote::default())],
            ..Default::default()
        };
        let reg = registry();
        let n = node(TASK, &task);
        let graph = wg::Graph::default();
        let seen = Observations::at(Utc::now());
        let exec = TaskExecutor::default();
        let r = run(1, "", vec![], &[]);
        let cx = ExecContext {
            graph: &graph,
            runs: &[],
            node: &n,
            run: &r,
            seen: &seen,
            extensions: &ext,
            registry: &reg,
        };
        let Action::StartSession { prompt, .. } = &exec.next(&cx)[0] else {
            panic!()
        };
        assert!(prompt.ends_with("[stack:executor]"));
        check(
            &exec,
            &ext,
            &task,
            vec![Case {
                name: "the hook holds completion",
                run: run(1, stage::AWAITING_MERGE, vec![], &[("u", "merged")]),
                sessions: vec![],
                verdicts: vec![],
                want: vec!["wait the release"],
            }],
        );
    }
}
