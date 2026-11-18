//! The registry of node and edge kinds every write is checked against.

use std::collections::HashMap;

use buffa::MessageName;
use factory_proto::workgraph::v1 as wg;

/// The type URL of a payload message.
pub fn type_url<M: MessageName>() -> &'static str {
    M::TYPE_URL
}

pub const PLAN: &str = wg::Plan::TYPE_URL;
pub const TASK: &str = wg::Task::TYPE_URL;
pub const DECISION: &str = wg::Decision::TYPE_URL;
pub const GATE: &str = wg::Gate::TYPE_URL;

pub const LEARNING: &str = wg::Learning::TYPE_URL;
pub const RECONCILE_NOTE: &str = wg::ReconcileNote::TYPE_URL;
pub const DECISION_OUTCOME: &str = wg::DecisionOutcome::TYPE_URL;

/// The hierarchy: the only TREE edge kind, and the shape of the Merkle tree.
pub const CHILD_OF: &str = "child_of";
/// Ordering: drives the ready set.
pub const BLOCKED_BY: &str = "blocked_by";
pub const RELATES_TO: &str = "relates_to";
pub const DISCOVERED_FROM: &str = "discovered_from";

/// The short name of a type URL (`Task` for `type.googleapis.com/…v1.Task`).
pub fn short_kind(kind: &str) -> &str {
    kind.rsplit(['/', '.']).next().unwrap_or(kind)
}

/// The schema with its lookups.
#[derive(Debug, Clone)]
pub struct Schema {
    proto: wg::Schema,
    nodes: HashMap<String, wg::NodeKind>,
    edges: HashMap<String, wg::EdgeKind>,
}

impl Schema {
    pub fn new(proto: wg::Schema) -> Self {
        let nodes = proto
            .node_kinds
            .iter()
            .map(|k| (k.kind.clone(), k.clone()))
            .collect();
        let edges = proto
            .edge_kinds
            .iter()
            .map(|k| (k.name.clone(), k.clone()))
            .collect();
        Self {
            proto,
            nodes,
            edges,
        }
    }

    /// The reference kind set alone: the built-in extension's schema.
    /// `FactoryBuilder::schema` merges it with the other extensions'.
    pub fn builtin() -> Self {
        crate::FactoryBuilder::new()
            .schema()
            .expect("the built-in extension merges")
    }

    pub fn proto(&self) -> &wg::Schema {
        &self.proto
    }

    /// The registered gate condition, by type URL.
    pub fn gate_condition(&self, type_url: &str) -> Option<&wg::PayloadKind> {
        self.proto
            .gate_conditions
            .iter()
            .find(|k| k.type_url == type_url)
    }

    /// The registered task extension, by type URL.
    pub fn task_extension(&self, type_url: &str) -> Option<&wg::PayloadKind> {
        self.proto
            .task_extensions
            .iter()
            .find(|k| k.type_url == type_url)
    }

    pub fn node_kind(&self, kind: &str) -> Option<&wg::NodeKind> {
        self.nodes.get(kind)
    }

    pub fn edge_kind(&self, name: &str) -> Option<&wg::EdgeKind> {
        self.edges.get(name)
    }

    pub fn edge_kinds(&self) -> impl Iterator<Item = &wg::EdgeKind> {
        self.proto.edge_kinds.iter()
    }

    /// The names of the edge kinds folded into node hashes, other than the
    /// tree itself.
    pub fn hashed_edge_kinds(&self) -> impl Iterator<Item = &str> {
        self.edge_kinds()
            .filter(|k| k.hashed && k.topology != wg::Topology::TOPOLOGY_TREE)
            .map(|k| k.name.as_str())
    }
}

/// Whether `kind` is in `allowed`, where an empty list allows any kind.
pub fn allows(allowed: &[String], kind: &str) -> bool {
    allowed.is_empty() || allowed.iter().any(|k| k == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_kind_strips_package() {
        assert_eq!(short_kind(TASK), "Task");
        assert_eq!(short_kind("Thing"), "Thing");
    }

    #[test]
    fn builtin_hashes_blockers_only() {
        let s = Schema::builtin();
        assert_eq!(s.hashed_edge_kinds().collect::<Vec<_>>(), [BLOCKED_BY]);
        assert!(s.node_kind(PLAN).unwrap().root);
    }
}
