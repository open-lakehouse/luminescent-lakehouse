//! Gate evaluation: what a [`GateEvaluator`] sees and says, and the
//! reference evaluators the built-in extension registers (`PrMerged`,
//! `HumanGate`, `NotBefore`).

use buffa_types::google::protobuf::Any;
use chrono::{DateTime, TimeZone, Utc};
use factory_proto::workgraph::v1 as wg;

use crate::extension::{GateEvaluator, check_any};
use crate::{Result, executor};

/// What an evaluator sees: the gate, its graph and runs, its attention
/// items, and the time.
pub struct GateContext<'a> {
    pub graph: &'a wg::Graph,
    pub runs: &'a [wg::Run],
    /// The gate node.
    pub gate: &'a wg::Node,
    /// The attention items raised for the gate node, resolved or not.
    pub attention: &'a [wg::Attention],
    pub now: DateTime<Utc>,
}

impl GateContext<'_> {
    pub fn node(&self, node_ref: &str) -> Option<&wg::Node> {
        self.graph.nodes.iter().find(|n| n.r#ref == node_ref)
    }
}

/// What an evaluator says about its gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateObservation {
    /// The gate holds no more: the node is done.
    Open { observed: String },
    /// Not yet; `observed` says what it waits on, for the canvas.
    Pending { observed: String },
    /// A person has to open it. The gate raises a `gate` attention item with
    /// `summary`, once.
    NeedsPerson { summary: String },
}

impl GateObservation {
    pub fn observed(&self) -> &str {
        match self {
            Self::Open { observed } | Self::Pending { observed } => observed,
            Self::NeedsPerson { summary } => summary,
        }
    }
}

fn pending(observed: impl Into<String>) -> GateObservation {
    GateObservation::Pending {
        observed: observed.into(),
    }
}

fn open(observed: impl Into<String>) -> GateObservation {
    GateObservation::Open {
        observed: observed.into(),
    }
}

/// `PrMerged`: every PR of the task's latest run is merged (or the task is
/// done).
pub struct PrMergedGate;

impl GateEvaluator for PrMergedGate {
    fn condition_type(&self) -> &str {
        wg::PrMerged::TYPE_URL
    }
    fn display_name(&self) -> &str {
        "PR merged"
    }
    fn description(&self) -> &str {
        "Every PR of a task's run is merged."
    }
    fn validate(&self, condition: &Any) -> Result<()> {
        check_any::<wg::PrMerged>(condition)
    }
    fn evaluate(&self, condition: &Any, cx: &GateContext<'_>) -> GateObservation {
        let Ok(cond) = condition.unpack_unchecked::<wg::PrMerged>() else {
            return pending("the condition doesn't decode");
        };
        if cond.task.is_empty() {
            return pending("the gate names no task");
        }
        let Some(task) = cx.node(&cond.task) else {
            return pending(format!("task {} isn't in the plan", cond.task));
        };
        let title = &task.spec.title;
        if executor::phase(task) == wg::Phase::PHASE_DONE {
            return open(format!("{title} is done"));
        }
        let Some(run) = executor::latest_run(cx.runs, &cond.task) else {
            return pending(format!("{title} hasn't run yet"));
        };
        let prs = executor::pull_requests(run);
        if prs.is_empty() {
            return pending(format!("{title} has no PRs yet"));
        }
        let merged = prs.iter().filter(|p| p.state == "merged").count();
        if merged == prs.len() {
            open(format!("{merged} PR(s) of {title} merged"))
        } else {
            pending(format!("{merged} of {} PRs of {title} merged", prs.len()))
        }
    }
}

/// `HumanGate`: open once a person accepts its attention item.
pub struct HumanGateEval;

impl GateEvaluator for HumanGateEval {
    fn condition_type(&self) -> &str {
        wg::HumanGate::TYPE_URL
    }
    fn display_name(&self) -> &str {
        "Person"
    }
    fn description(&self) -> &str {
        "A person checks something and opens the gate."
    }
    fn validate(&self, condition: &Any) -> Result<()> {
        check_any::<wg::HumanGate>(condition)
    }
    fn evaluate(&self, condition: &Any, cx: &GateContext<'_>) -> GateObservation {
        let prompt = condition
            .unpack_unchecked::<wg::HumanGate>()
            .map(|g| g.prompt)
            .unwrap_or_default();
        let confirmed = cx.attention.iter().find(|a| {
            a.kind.as_known() == Some(wg::AttentionKind::ATTENTION_KIND_GATE)
                && a.resolved
                && a.resolution == "accept"
        });
        match confirmed {
            Some(a) if a.resolved_by.is_empty() => open("confirmed"),
            Some(a) => open(format!("confirmed by {}", a.resolved_by)),
            None => GateObservation::NeedsPerson {
                summary: if prompt.is_empty() {
                    format!("Confirm the gate {}", cx.gate.spec.title)
                } else {
                    prompt
                },
            },
        }
    }
}

/// `NotBefore`: open from a point in time.
pub struct NotBeforeGate;

impl GateEvaluator for NotBeforeGate {
    fn condition_type(&self) -> &str {
        wg::NotBefore::TYPE_URL
    }
    fn display_name(&self) -> &str {
        "Not before"
    }
    fn description(&self) -> &str {
        "Opens at a point in time."
    }
    fn validate(&self, condition: &Any) -> Result<()> {
        check_any::<wg::NotBefore>(condition)
    }
    fn evaluate(&self, condition: &Any, cx: &GateContext<'_>) -> GateObservation {
        let at = condition
            .unpack_unchecked::<wg::NotBefore>()
            .ok()
            .and_then(|c| c.at.into_option())
            .and_then(|t| Utc.timestamp_opt(t.seconds, 0).single());
        match at {
            None => pending("no time set"),
            Some(at) if cx.now >= at => open(format!("opened at {}", at.to_rfc3339())),
            Some(at) => pending(format!("opens at {}", at.to_rfc3339())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::pack;
    use buffa_types::google::protobuf::Timestamp;

    fn node(r: &str, kind: &str, phase: wg::Phase) -> wg::Node {
        wg::Node {
            r#ref: r.into(),
            kind: kind.into(),
            spec: wg::NodeSpec {
                title: format!("node {r}"),
                ..Default::default()
            }
            .into(),
            status: wg::NodeStatus {
                phase: phase.into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
    }

    fn pr(state: &str) -> wg::Artifact {
        wg::Artifact {
            artifact: Some(wg::artifact::Artifact::PullRequest(Box::new(
                wg::PullRequest {
                    url: format!("u-{state}"),
                    state: state.into(),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        }
    }

    fn run(node: &str, attempt: u32, prs: &[&str]) -> wg::Run {
        wg::Run {
            r#ref: format!("run-{attempt}"),
            node: node.into(),
            attempt,
            artifacts: prs.iter().map(|s| pr(s)).collect(),
            ..Default::default()
        }
    }

    fn attention(resolution: &str, resolved: bool) -> wg::Attention {
        wg::Attention {
            kind: wg::AttentionKind::ATTENTION_KIND_GATE.into(),
            resolved,
            resolution: resolution.into(),
            resolved_by: "ops".into(),
            ..Default::default()
        }
    }

    /// (condition, task phase, runs, attention, now) → observation.
    #[test]
    fn evaluators() {
        let now = Utc.timestamp_opt(1_000, 0).unwrap();
        let merged = pack(&wg::PrMerged {
            task: "t".into(),
            ..Default::default()
        });
        let human = pack(&wg::HumanGate {
            prompt: "Check the release notes".into(),
            ..Default::default()
        });
        let at = |s: i64| {
            pack(&wg::NotBefore {
                at: Timestamp {
                    seconds: s,
                    nanos: 0,
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            })
        };
        type Case<'a> = (
            &'a str,
            Any,
            wg::Phase,
            Vec<wg::Run>,
            Vec<wg::Attention>,
            GateObservation,
        );
        let cases: Vec<Case> = vec![
            (
                "no run yet",
                merged.clone(),
                wg::Phase::PHASE_APPROVED,
                vec![],
                vec![],
                pending("node t hasn't run yet"),
            ),
            (
                "no PRs",
                merged.clone(),
                wg::Phase::PHASE_APPROVED,
                vec![run("t", 1, &[])],
                vec![],
                pending("node t has no PRs yet"),
            ),
            (
                "some merged",
                merged.clone(),
                wg::Phase::PHASE_APPROVED,
                vec![run("t", 1, &["merged", "open"])],
                vec![],
                pending("1 of 2 PRs of node t merged"),
            ),
            (
                "all merged, latest attempt counts",
                merged.clone(),
                wg::Phase::PHASE_APPROVED,
                vec![run("t", 1, &["open"]), run("t", 2, &["merged"])],
                vec![],
                open("1 PR(s) of node t merged"),
            ),
            (
                "task done",
                merged.clone(),
                wg::Phase::PHASE_DONE,
                vec![],
                vec![],
                open("node t is done"),
            ),
            (
                "a person is asked",
                human.clone(),
                wg::Phase::PHASE_APPROVED,
                vec![],
                vec![],
                GateObservation::NeedsPerson {
                    summary: "Check the release notes".into(),
                },
            ),
            (
                "dismissed isn't confirmed",
                human.clone(),
                wg::Phase::PHASE_APPROVED,
                vec![],
                vec![attention("dismiss", true)],
                GateObservation::NeedsPerson {
                    summary: "Check the release notes".into(),
                },
            ),
            (
                "confirmed",
                human.clone(),
                wg::Phase::PHASE_APPROVED,
                vec![],
                vec![attention("accept", true)],
                open("confirmed by ops"),
            ),
            (
                "before",
                at(2_000),
                wg::Phase::PHASE_APPROVED,
                vec![],
                vec![],
                pending("opens at 1970-01-01T00:33:20+00:00"),
            ),
            (
                "at",
                at(1_000),
                wg::Phase::PHASE_APPROVED,
                vec![],
                vec![],
                open("opened at 1970-01-01T00:16:40+00:00"),
            ),
            (
                "no time",
                pack(&wg::NotBefore::default()),
                wg::Phase::PHASE_APPROVED,
                vec![],
                vec![],
                pending("no time set"),
            ),
        ];
        let evaluators: Vec<Box<dyn GateEvaluator>> = vec![
            Box::new(PrMergedGate),
            Box::new(HumanGateEval),
            Box::new(NotBeforeGate),
        ];
        for (name, condition, phase, runs, attention, want) in cases {
            let gate = node("g", crate::schema::GATE, wg::Phase::PHASE_APPROVED);
            let graph = wg::Graph {
                nodes: vec![node("t", crate::schema::TASK, phase), gate.clone()],
                ..Default::default()
            };
            let cx = GateContext {
                graph: &graph,
                runs: &runs,
                gate: &gate,
                attention: &attention,
                now,
            };
            let e = evaluators
                .iter()
                .find(|e| e.condition_type() == condition.type_url)
                .unwrap();
            assert_eq!(e.evaluate(&condition, &cx), want, "{name}");
        }
    }

    #[test]
    fn a_declared_condition_holds() {
        let declared = crate::extension::Declared::<wg::Learning>::new("Learning", "");
        let graph = wg::Graph::default();
        let gate = wg::Node::default();
        let cx = GateContext {
            graph: &graph,
            runs: &[],
            gate: &gate,
            attention: &[],
            now: Utc::now(),
        };
        let obs = declared.evaluate(&pack(&wg::Learning::default()), &cx);
        assert_eq!(obs, pending("Learning has no evaluator yet"));
    }
}
