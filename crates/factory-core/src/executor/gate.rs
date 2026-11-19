//! `gate.v1`: no session. It asks the gate's condition evaluator on every
//! pass, records what it waits on, and marks the node done when it opens. A
//! condition that needs a person raises one `gate` attention item.

use factory_proto::workgraph::v1 as wg;

use super::{Action, ExecContext, has_open, stage};
use crate::extension::NodeExecutor;
use crate::gates::{GateContext, GateObservation};
use crate::schema::GATE;

pub const NAME: &str = "gate.v1";

/// The reference `gate.v1` executor.
#[derive(Debug, Clone, Default)]
pub struct GateExecutor;

impl NodeExecutor for GateExecutor {
    fn kind(&self) -> &str {
        GATE
    }

    fn name(&self) -> &str {
        NAME
    }

    fn next(&self, cx: &ExecContext<'_>) -> Vec<Action> {
        let gate: wg::Gate = cx.payload();
        let observation = match gate.condition.as_option() {
            None => GateObservation::Pending {
                observed: "the gate has no condition yet".into(),
            },
            Some(condition) => match cx.extensions.gate(&condition.type_url) {
                None => GateObservation::Pending {
                    observed: format!("no evaluator for {}", condition.type_url),
                },
                Some(evaluator) => evaluator.evaluate(
                    condition,
                    &GateContext {
                        graph: cx.graph,
                        runs: cx.runs,
                        gate: cx.node,
                        attention: &cx.seen.attention,
                        now: cx.seen.now,
                    },
                ),
            },
        };
        let mut out = Vec::new();
        if cx.run.stage != stage::AWAITING_GATE {
            out.push(Action::SetStage(stage::AWAITING_GATE.into()));
        }
        out.push(Action::Wait {
            observed: observation.observed().to_owned(),
        });
        match observation {
            GateObservation::Open { .. } => out.push(Action::Complete { node_done: true }),
            GateObservation::Pending { .. } => {}
            GateObservation::NeedsPerson { summary } => {
                let kind = wg::AttentionKind::ATTENTION_KIND_GATE;
                if !has_open(cx.seen, cx.run, kind) {
                    out.push(Action::RaiseAttention {
                        kind,
                        summary,
                        step: None,
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::Observations;
    use crate::executor::testkit::{node, run};
    use crate::payload::pack;
    use chrono::{TimeZone, Utc};

    fn next(gate: &wg::Gate, stage_: &str, attention: Vec<wg::Attention>) -> Vec<Action> {
        let n = node(GATE, gate);
        let graph = wg::Graph {
            nodes: vec![n.clone()],
            ..Default::default()
        };
        let mut seen = Observations::at(Utc.timestamp_opt(1_000, 0).unwrap());
        seen.attention = attention;
        let r = run(1, stage_, vec![], &[]);
        let ext = crate::Extensions::builtin();
        let reg = crate::Registry::default();
        GateExecutor.next(&ExecContext {
            graph: &graph,
            runs: &[],
            node: &n,
            run: &r,
            seen: &seen,
            extensions: &ext,
            registry: &reg,
        })
    }

    fn gate(condition: Option<buffa_types::google::protobuf::Any>) -> wg::Gate {
        wg::Gate {
            condition: condition.into(),
            ..Default::default()
        }
    }

    fn wait(s: &str) -> Action {
        Action::Wait { observed: s.into() }
    }

    #[test]
    fn transitions() {
        let human = gate(Some(pack(&wg::HumanGate {
            prompt: "ok?".into(),
            ..Default::default()
        })));
        let ask = Action::RaiseAttention {
            kind: wg::AttentionKind::ATTENTION_KIND_GATE,
            summary: "ok?".into(),
            step: None,
        };
        let open_item = wg::Attention {
            run: "run".into(),
            kind: wg::AttentionKind::ATTENTION_KIND_GATE.into(),
            ..Default::default()
        };
        let accepted = wg::Attention {
            resolved: true,
            resolution: "accept".into(),
            ..open_item.clone()
        };
        let past = gate(Some(pack(&wg::NotBefore {
            at: buffa_types::google::protobuf::Timestamp {
                seconds: 10,
                ..Default::default()
            }
            .into(),
            ..Default::default()
        })));
        let stage_ = Action::SetStage(stage::AWAITING_GATE.into());
        let cases: Vec<(&str, Vec<Action>, Vec<Action>)> = vec![
            (
                "a draft gate waits",
                next(&gate(None), "", vec![]),
                vec![stage_.clone(), wait("the gate has no condition yet")],
            ),
            (
                "an unknown condition waits",
                next(
                    &gate(Some(pack(&wg::Learning::default()))),
                    stage::AWAITING_GATE,
                    vec![],
                ),
                vec![wait(
                    "no evaluator for type.googleapis.com/factory.workgraph.v1.Learning",
                )],
            ),
            (
                "a person is asked once",
                next(&human, stage::AWAITING_GATE, vec![]),
                vec![wait("ok?"), ask],
            ),
            (
                "and not again",
                next(&human, stage::AWAITING_GATE, vec![open_item]),
                vec![wait("ok?")],
            ),
            (
                "confirmed opens",
                next(&human, stage::AWAITING_GATE, vec![accepted]),
                vec![wait("confirmed"), Action::Complete { node_done: true }],
            ),
            (
                "a time in the past opens",
                next(&past, "", vec![]),
                vec![
                    stage_,
                    wait("opened at 1970-01-01T00:00:10+00:00"),
                    Action::Complete { node_done: true },
                ],
            ),
        ];
        for (name, got, want) in cases {
            assert_eq!(got, want, "{name}");
        }
    }
}
