use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::hash::{fold_subtree, hash_parts};

/// Why a tree can't be built.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TreeError {
    /// A content part contained NUL, the part separator.
    #[error("content part {index} contains NUL")]
    NulInPart { index: usize },
    /// Two nodes share a key, so a diff couldn't tell them apart.
    #[error("duplicate node key {0:?}")]
    DuplicateKey(String),
}

/// Whether the order of a node's children is part of the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChildOrder {
    /// Children keep the order they're given in, it's folded into the subtree
    /// hash, and a changed position counts as a move (docs-factory: document
    /// order).
    #[default]
    Ordered,
    /// Children are a set: they're sorted by key before hashing, and only a
    /// change of parent counts as a move (the factory's work graph).
    SortedByKey,
}

/// A node to hash: its identity, its own content, and its children.
#[derive(Debug, Clone, Default)]
pub struct NodeInput {
    /// The stable key a diff matches nodes by. Unique within the tree.
    pub key: String,
    /// What the node is, for display and for the diff's `kind`.
    pub kind: String,
    /// A display label.
    pub label: String,
    /// The node's own content, kind tag first. Hashed, in order.
    pub parts: Vec<String>,
    /// Domain fields carried on the output node and never hashed.
    pub attrs: Map<String, Value>,
    pub children: Vec<NodeInput>,
}

impl NodeInput {
    pub fn new(key: impl Into<String>, kind: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            kind: kind.into(),
            label: label.into(),
            ..Self::default()
        }
    }

    pub fn parts<I, S>(mut self, parts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.parts = parts.into_iter().map(Into::into).collect();
        self
    }

    pub fn attr(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attrs.insert(name.into(), value.into());
        self
    }

    pub fn child(mut self, child: NodeInput) -> Self {
        self.children.push(child);
        self
    }
}

/// A hashed node, in docs-factory's `MerkleNodeJson` shape: camelCase
/// `nodeHash` and `subtreeHash`, and domain attributes (`level`,
/// `anchorSlug`, …) flattened next to them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MerkleNode {
    pub key: String,
    pub kind: String,
    pub node_hash: String,
    pub subtree_hash: String,
    pub label: String,
    #[serde(default)]
    pub children: Vec<MerkleNode>,
    #[serde(flatten)]
    pub attrs: Map<String, Value>,
}

impl MerkleNode {
    /// Every node below this one, depth first, in child order.
    pub fn descendants(&self) -> impl Iterator<Item = &MerkleNode> {
        let mut stack: Vec<&MerkleNode> = self.children.iter().rev().collect();
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children.iter().rev());
            Some(node)
        })
    }

    /// The subtree hash recomputed from the node hashes below, without
    /// trusting the stored subtree hashes. Equal to `subtree_hash` for a tree
    /// that hasn't been tampered with.
    pub fn recompute_subtree_hash(&self) -> String {
        let children: Vec<String> = self
            .children
            .iter()
            .map(MerkleNode::recompute_subtree_hash)
            .collect();
        fold_subtree(&self.node_hash, &children)
    }
}

/// Hash `root` bottom-up.
pub fn build(root: NodeInput, order: ChildOrder) -> Result<MerkleNode, TreeError> {
    let mut seen = HashSet::new();
    build_node(root, order, &mut seen)
}

fn build_node(
    input: NodeInput,
    order: ChildOrder,
    seen: &mut HashSet<String>,
) -> Result<MerkleNode, TreeError> {
    if !seen.insert(input.key.clone()) {
        return Err(TreeError::DuplicateKey(input.key));
    }
    let node_hash = hash_parts(&input.parts)?;
    let mut children = input
        .children
        .into_iter()
        .map(|c| build_node(c, order, seen))
        .collect::<Result<Vec<_>, _>>()?;
    if order == ChildOrder::SortedByKey {
        children.sort_by(|a, b| a.key.cmp(&b.key));
    }
    let child_hashes: Vec<&str> = children.iter().map(|c| c.subtree_hash.as_str()).collect();
    let subtree_hash = fold_subtree(&node_hash, &child_hashes);
    Ok(MerkleNode {
        key: input.key,
        kind: input.kind,
        node_hash,
        subtree_hash,
        label: input.label,
        children,
        attrs: input.attrs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, text: &str) -> NodeInput {
        NodeInput::new(key, "item", key).parts(["item", text])
    }

    #[test]
    fn sorted_children_hash_independently_of_input_order() {
        let a = NodeInput::new("r", "plan", "r")
            .parts(["plan"])
            .child(item("a", "1"))
            .child(item("b", "2"));
        let b = NodeInput::new("r", "plan", "r")
            .parts(["plan"])
            .child(item("b", "2"))
            .child(item("a", "1"));
        let ta = build(a.clone(), ChildOrder::SortedByKey).unwrap();
        let tb = build(b.clone(), ChildOrder::SortedByKey).unwrap();
        assert_eq!(ta.subtree_hash, tb.subtree_hash);
        assert_eq!(ta.children[0].key, "a");

        let oa = build(a, ChildOrder::Ordered).unwrap();
        let ob = build(b, ChildOrder::Ordered).unwrap();
        assert_ne!(oa.subtree_hash, ob.subtree_hash);
    }

    #[test]
    fn a_change_moves_up_to_the_root_only() {
        let before = NodeInput::new("r", "plan", "r")
            .parts(["plan"])
            .child(item("a", "1").child(item("a1", "x")))
            .child(item("b", "2"));
        let after = NodeInput::new("r", "plan", "r")
            .parts(["plan"])
            .child(item("a", "1").child(item("a1", "y")))
            .child(item("b", "2"));
        let tb = build(before, ChildOrder::SortedByKey).unwrap();
        let ta = build(after, ChildOrder::SortedByKey).unwrap();
        assert_ne!(tb.subtree_hash, ta.subtree_hash);
        assert_eq!(tb.node_hash, ta.node_hash);
        assert_eq!(tb.children[0].node_hash, ta.children[0].node_hash);
        assert_ne!(tb.children[0].subtree_hash, ta.children[0].subtree_hash);
        assert_eq!(tb.children[1], ta.children[1]);
    }

    #[test]
    fn rejects_duplicate_keys() {
        let t = NodeInput::new("r", "plan", "r")
            .child(item("a", "1"))
            .child(item("a", "2"));
        assert_eq!(
            build(t, ChildOrder::Ordered),
            Err(TreeError::DuplicateKey("a".into()))
        );
    }

    #[test]
    fn json_shape_flattens_attrs() {
        let t = build(
            NodeInput::new("r", "doc", "(document)")
                .parts(["doc"])
                .attr("level", 0),
            ChildOrder::Ordered,
        )
        .unwrap();
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["nodeHash"], json["subtreeHash"]);
        assert_eq!(json["level"], 0);
        let back: MerkleNode = serde_json::from_value(json).unwrap();
        assert_eq!(back, t);
    }
}
