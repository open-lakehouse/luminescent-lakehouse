//! The built-in extension: the reference kind set every factory has.
//!
//! `Plan` at the root, `Task`, `Decision` and `Gate` below it, the four
//! built-in edge kinds, and the generic gate conditions: `PrMerged`,
//! `HumanGate` and `NotBefore`. A `Task` takes no extensions of its own;
//! every one comes from another extension's task hook.

use buffa::Message;
use factory_proto::workgraph::v1 as wg;

use crate::extension::{Declared, Extension, Extensions, GateEvaluator, KindSpec};
use crate::schema::{BLOCKED_BY, CHILD_OF, DECISION, DISCOVERED_FROM, GATE, RELATES_TO, TASK};
use crate::{Error, Result};

/// The name the built-in extension registers under.
pub const NAME: &str = "builtin";

/// The reference kind set. [`FactoryBuilder::new`](crate::FactoryBuilder::new)
/// always includes it.
pub struct Builtin;

fn display(icon: &str, tone: &str, card_fields: &[&str]) -> wg::KindDisplay {
    wg::KindDisplay {
        icon: icon.into(),
        tone: tone.into(),
        card_fields: card_fields.iter().map(|f| (*f).to_owned()).collect(),
        ..Default::default()
    }
}

fn node(
    name: &str,
    root: bool,
    children: &[String],
    desc: &str,
    display: wg::KindDisplay,
) -> wg::NodeKind {
    wg::NodeKind {
        display_name: name.into(),
        root,
        allowed_child_kinds: children.to_vec(),
        description: desc.into(),
        display: display.into(),
        ..Default::default()
    }
}

fn edge(
    name: &str,
    inverse: &str,
    topology: wg::Topology,
    from: &[String],
    to: &[String],
    hashed: bool,
    desc: &str,
) -> wg::EdgeKind {
    wg::EdgeKind {
        name: name.into(),
        inverse: inverse.into(),
        topology: topology.into(),
        from_kinds: from.to_vec(),
        to_kinds: to.to_vec(),
        hashed,
        description: desc.into(),
        ..Default::default()
    }
}

fn items() -> Vec<String> {
    vec![TASK.to_owned(), DECISION.to_owned(), GATE.to_owned()]
}

/// A gate decodes, and its condition (if set) is a registered one that
/// checks out. A gate with no condition yet is a draft: it's stored, and
/// waits until it gets one.
fn validate_gate(bytes: &[u8], ext: &Extensions) -> Result<()> {
    let gate = wg::Gate::decode_from_slice(bytes)
        .map_err(|_| Error::invalid("payload doesn't decode as a Gate"))?;
    match gate.condition.as_option() {
        Some(condition) => ext.validate_condition(condition),
        None => Ok(()),
    }
}

/// A task decodes, and each of its extensions is a registered one that
/// checks out.
fn validate_task(bytes: &[u8], ext: &Extensions) -> Result<()> {
    let task = wg::Task::decode_from_slice(bytes)
        .map_err(|_| Error::invalid("payload doesn't decode as a Task"))?;
    task.extensions
        .iter()
        .try_for_each(|e| ext.validate_task_extension(e))
}

impl Extension for Builtin {
    fn name(&self) -> &'static str {
        NAME
    }

    fn kinds(&self) -> Vec<KindSpec> {
        let items = items();
        vec![
            KindSpec::of::<wg::Plan>(node(
                "Plan",
                true,
                &items,
                "The root of a plan: its goal and rubric.",
                display("map", "violet", &["goal"]),
            )),
            KindSpec::with(
                wg::NodeKind {
                    kind: TASK.into(),
                    ..node(
                        "Task",
                        false,
                        &items,
                        "One unit of work: one session, one PR.",
                        display("list-checks", "sky", &["repo"]),
                    )
                },
                validate_task,
            ),
            KindSpec::of::<wg::Decision>(node(
                "Decision",
                false,
                &[],
                "A question the plan can't go past without an answer.",
                display("split", "amber", &["outcome"]),
            )),
            KindSpec::with(
                wg::NodeKind {
                    kind: GATE.into(),
                    ..node(
                        "Gate",
                        false,
                        &[],
                        "A condition the plan waits on: a merge, a person, a time.",
                        display("hourglass", "slate", &[]),
                    )
                },
                validate_gate,
            ),
        ]
    }

    fn edge_kinds(&self) -> Vec<wg::EdgeKind> {
        let any: Vec<String> = Vec::new();
        let items = items();
        vec![
            edge(
                CHILD_OF,
                "parent_of",
                wg::Topology::TOPOLOGY_TREE,
                &items,
                &any,
                true,
                "The hierarchy. One parent per node.",
            ),
            edge(
                BLOCKED_BY,
                "blocks",
                wg::Topology::TOPOLOGY_DAG,
                &items,
                &items,
                true,
                "Ordering: the source waits for the target.",
            ),
            edge(
                RELATES_TO,
                "relates_to",
                wg::Topology::TOPOLOGY_NETWORK,
                &any,
                &any,
                false,
                "A cross-reference. Not plan content.",
            ),
            edge(
                DISCOVERED_FROM,
                "led_to",
                wg::Topology::TOPOLOGY_NETWORK,
                &items,
                &items,
                false,
                "Found while working on the target.",
            ),
        ]
    }

    fn gates(&self) -> Vec<Box<dyn GateEvaluator>> {
        vec![
            Box::new(Declared::<wg::PrMerged>::new(
                "PR merged",
                "Every PR of a task's run is merged.",
            )),
            Box::new(Declared::<wg::HumanGate>::new(
                "Person",
                "A person checks something and opens the gate.",
            )),
            Box::new(Declared::<wg::NotBefore>::new(
                "Not before",
                "Opens at a point in time.",
            )),
        ]
    }

    fn descriptors(&self) -> &'static [u8] {
        factory_proto::DESCRIPTORS
    }
}
