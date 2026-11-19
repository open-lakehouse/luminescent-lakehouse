//! `plan.v1`: planning. A planner session works on the plan root through the
//! work-graph MCP; when it's idle, the run completes and an `approval` item
//! asks a person to review the revision. The root itself stays open.

use factory_proto::workgraph::v1 as wg;

use super::{
    Action, ExecContext, follow_ups, last_step, profile_or_default, role, stage, stall, step,
};
use crate::extension::NodeExecutor;
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
        let Some(planning) = last_step(cx.run, step::PLAN) else {
            return vec![
                Action::StartSession {
                    step: step::PLAN.into(),
                    role: role::PLANNER.into(),
                    profile: profile_or_default(&plan.planner_profile, role::PLANNER, cx.registry),
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
        if !seen.done(follow_ups(planning)) {
            return Vec::new();
        }
        vec![
            Action::SetStep {
                step: planning.index,
                state: wg::StepState::STEP_STATE_SUCCEEDED,
            },
            Action::RaiseAttention {
                kind: wg::AttentionKind::ATTENTION_KIND_APPROVAL,
                summary: format!("Review the plan {}", cx.node.spec.title),
                step: Some(planning.index),
            },
            Action::Complete { node_done: false },
        ]
    }
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::SessionStatus::{Failed, Idle, Running};
    use crate::executor::Observations;
    use crate::executor::testkit::{node, run, session_step, view};
    use chrono::Utc;

    #[test]
    fn transitions() {
        let reg =
            crate::Registry::from_yaml("defaults: {planner: plan}\nprofiles: {plan: {agent: p}}\n")
                .unwrap();
        let ext = crate::Extensions::builtin();
        let n = node(
            PLAN,
            &wg::Plan {
                goal: "Ship it".into(),
                ..Default::default()
            },
        );
        let graph = wg::Graph::default();
        let planning = || {
            run(
                1,
                stage::PLANNING,
                vec![session_step(0, step::PLAN, 0)],
                &[],
            )
        };
        type Case = (
            &'static str,
            wg::Run,
            Vec<(u32, crate::executor::SessionView)>,
            usize,
            &'static str,
        );
        let cases: Vec<Case> = vec![
            ("start", run(1, "", vec![], &[]), vec![], 2, "StartSession"),
            (
                "running waits",
                planning(),
                vec![(0, view(Running, 1))],
                0,
                "",
            ),
            (
                "idle asks for approval",
                planning(),
                vec![(0, view(Idle, 1))],
                3,
                "Complete { node_done: false }",
            ),
            (
                "failed retries",
                planning(),
                vec![(0, view(Failed, 1))],
                1,
                "Retry",
            ),
        ];
        for (name, r, sessions, len, has) in cases {
            let mut seen = Observations::at(Utc::now());
            seen.sessions = sessions.into_iter().collect();
            let got = PlanExecutor::default().next(&ExecContext {
                graph: &graph,
                runs: &[],
                node: &n,
                run: &r,
                seen: &seen,
                extensions: &ext,
                registry: &reg,
            });
            assert_eq!(got.len(), len, "{name}: {got:?}");
            assert!(format!("{got:?}").contains(has), "{name}: {got:?}");
        }
    }
}
