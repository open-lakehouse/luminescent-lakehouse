use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::tree::{ChildOrder, MerkleNode};

/// How a node differs between two trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Change {
    /// Only in the later tree.
    Added,
    /// Only in the earlier tree.
    Removed,
    /// The node's own content changed.
    Modified,
    /// The node's own content is the same; something below it changed.
    ModifiedDescendants,
    /// The node has a new parent (or, with [`ChildOrder::Ordered`], a new
    /// position). Wins over `Modified`; see [`DiffEntry::content_changed`].
    Moved,
}

/// One changed node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    pub key: String,
    pub kind: String,
    pub change: Change,
    pub label: String,
    /// Whether the node's own content changed. Always true for `Modified`;
    /// for `Moved` it tells a move apart from a move plus an edit.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub content_changed: bool,
    /// The parent before a move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_parent: Option<String>,
    /// The parent after a move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_parent: Option<String>,
}

struct Indexed<'a> {
    node: &'a MerkleNode,
    parent_key: &'a str,
    ordinal: usize,
}

/// Every node below the root, in depth-first order, with its parent and
/// position. The root itself is never a diff entry.
fn index(root: &MerkleNode) -> (Vec<&str>, HashMap<&str, Indexed<'_>>) {
    fn walk<'a>(
        node: &'a MerkleNode,
        order: &mut Vec<&'a str>,
        map: &mut HashMap<&'a str, Indexed<'a>>,
    ) {
        for (ordinal, child) in node.children.iter().enumerate() {
            order.push(&child.key);
            map.insert(
                &child.key,
                Indexed {
                    node: child,
                    parent_key: &node.key,
                    ordinal,
                },
            );
            walk(child, order, map);
        }
    }
    let mut order = Vec::new();
    let mut map = HashMap::new();
    walk(root, &mut order, &mut map);
    (order, map)
}

fn entry(node: &MerkleNode, change: Change) -> DiffEntry {
    DiffEntry {
        key: node.key.clone(),
        kind: node.kind.clone(),
        change,
        label: node.label.clone(),
        content_changed: change == Change::Modified,
        from_parent: None,
        to_parent: None,
    }
}

/// Diff two trees by key. Returns one entry per changed node, later-tree
/// nodes first in depth-first order, then removed nodes; unchanged subtrees
/// are pruned. Empty when the roots' subtree hashes are equal.
///
/// A missing `before` makes every node added; a missing `after` yields
/// nothing, as in docs-factory.
pub fn diff(
    before: Option<&MerkleNode>,
    after: Option<&MerkleNode>,
    order: ChildOrder,
) -> Vec<DiffEntry> {
    let Some(after) = after else {
        return Vec::new();
    };
    let (b_order, b) = index(after);
    let Some(before) = before else {
        return b_order
            .iter()
            .map(|k| entry(b[k].node, Change::Added))
            .collect();
    };
    if before.subtree_hash == after.subtree_hash {
        return Vec::new();
    }
    let (a_order, a) = index(before);

    let mut out = Vec::new();
    for key in &b_order {
        let cur = &b[key];
        let Some(prev) = a.get(key) else {
            out.push(entry(cur.node, Change::Added));
            continue;
        };
        let reparented = prev.parent_key != cur.parent_key;
        let moved = reparented || (order == ChildOrder::Ordered && prev.ordinal != cur.ordinal);
        let own_changed = prev.node.node_hash != cur.node.node_hash;
        let change = if prev.node.subtree_hash == cur.node.subtree_hash {
            if !moved {
                continue;
            }
            Change::Moved
        } else if moved {
            Change::Moved
        } else if own_changed {
            Change::Modified
        } else {
            Change::ModifiedDescendants
        };
        let mut e = entry(cur.node, change);
        e.content_changed = own_changed;
        if change == Change::Moved {
            e.from_parent = Some(prev.parent_key.to_owned());
            e.to_parent = Some(cur.parent_key.to_owned());
        }
        out.push(e);
    }
    for key in &a_order {
        if !b.contains_key(key) {
            out.push(entry(a[key].node, Change::Removed));
        }
    }
    out
}

/// Reduce a diff to review-level changes: drop descendant-only entries, and
/// represent an added or removed subtree by its topmost node.
pub fn compact(
    entries: &[DiffEntry],
    before: Option<&MerkleNode>,
    after: Option<&MerkleNode>,
) -> Vec<DiffEntry> {
    let before_index = before.map(|t| index(t).1).unwrap_or_default();
    let after_index = after.map(|t| index(t).1).unwrap_or_default();
    let changed: HashMap<&str, Change> =
        entries.iter().map(|e| (e.key.as_str(), e.change)).collect();

    entries
        .iter()
        .filter(|e| match e.change {
            Change::ModifiedDescendants => false,
            Change::Added | Change::Removed => {
                let index = if e.change == Change::Removed {
                    &before_index
                } else {
                    &after_index
                };
                let mut parent = index.get(e.key.as_str()).map(|i| i.parent_key);
                while let Some(p) = parent {
                    if changed.get(p) == Some(&e.change) {
                        return false;
                    }
                    parent = index.get(p).map(|i| i.parent_key);
                }
                true
            }
            Change::Modified | Change::Moved => true,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    //! The cases of docs-factory's `server/src/tree-diff.test.ts`, on
    //! hand-written hashes, plus the factory's key-sorted mode.

    use serde_json::Map;

    use super::*;

    fn node(key: &str, kind: &str, node_hash: &str, subtree_hash: &str) -> MerkleNode {
        MerkleNode {
            key: key.into(),
            kind: kind.into(),
            node_hash: node_hash.into(),
            subtree_hash: subtree_hash.into(),
            label: key.into(),
            children: Vec::new(),
            attrs: Map::new(),
        }
    }

    /// A heading with a prose child, as the TS fixtures build them.
    fn heading(
        key: &str,
        subtree: &str,
        prose: Option<&str>,
        children: Vec<MerkleNode>,
    ) -> MerkleNode {
        let prose_hash = prose.map_or_else(|| format!("{key}-prose"), str::to_owned);
        let mut h = node(key, "heading", &format!("{key}-node"), subtree);
        h.children.push(node(
            &format!("{key}#prose"),
            "prose",
            &prose_hash,
            &prose_hash,
        ));
        h.children.extend(children);
        h
    }

    fn doc(children: Vec<MerkleNode>, subtree: &str) -> MerkleNode {
        let mut d = node("", "doc", "doc", subtree);
        d.children = children;
        d
    }

    fn by_key(entries: &[DiffEntry]) -> HashMap<&str, Change> {
        entries.iter().map(|e| (e.key.as_str(), e.change)).collect()
    }

    const O: ChildOrder = ChildOrder::Ordered;

    #[test]
    fn identical_roots_fast_out() {
        let t = doc(vec![heading("intro", "s1", None, vec![])], "root1");
        assert!(diff(Some(&t), Some(&t), O).is_empty());
    }

    #[test]
    fn null_baseline_adds_everything() {
        let t = doc(vec![heading("intro", "s1", None, vec![])], "root1");
        let changes = diff(None, Some(&t), O);
        assert!(changes.iter().all(|c| c.change == Change::Added));
        let mut keys: Vec<_> = changes.iter().map(|c| c.key.as_str()).collect();
        keys.sort();
        assert_eq!(keys, ["intro", "intro#prose"]);
        assert!(diff(Some(&t), None, O).is_empty());
    }

    #[test]
    fn added_and_removed_sections() {
        let before = doc(vec![heading("intro", "s1", None, vec![])], "r1");
        let after = doc(
            vec![
                heading("intro", "s1", None, vec![]),
                heading("usage", "s2", None, vec![]),
            ],
            "r2",
        );
        let changes = diff(Some(&before), Some(&after), O);
        let m = by_key(&changes);
        assert_eq!(m.get("usage"), Some(&Change::Added));
        assert_eq!(m.get("intro"), None);

        let back = diff(Some(&after), Some(&before), O);
        assert_eq!(by_key(&back).get("usage"), Some(&Change::Removed));
    }

    #[test]
    fn own_content_vs_descendants() {
        let setup_before = heading("intro/setup", "setup-s1", Some("p1"), vec![]);
        let setup_after = heading("intro/setup", "setup-s2", Some("p2"), vec![]);
        let before = doc(
            vec![heading("intro", "intro-s1", None, vec![setup_before])],
            "r1",
        );
        let after = doc(
            vec![heading("intro", "intro-s2", None, vec![setup_after])],
            "r2",
        );
        let changes = diff(Some(&before), Some(&after), O);
        let m = by_key(&changes);
        assert_eq!(m["intro/setup#prose"], Change::Modified);
        assert_eq!(m["intro/setup"], Change::ModifiedDescendants);
        assert_eq!(m["intro"], Change::ModifiedDescendants);
    }

    #[test]
    fn swapped_siblings_move_only_when_ordered() {
        let before = doc(
            vec![
                heading("a", "sa", None, vec![]),
                heading("b", "sb", None, vec![]),
            ],
            "r1",
        );
        let after = doc(
            vec![
                heading("b", "sb", None, vec![]),
                heading("a", "sa", None, vec![]),
            ],
            "r2",
        );
        let m = diff(Some(&before), Some(&after), O);
        assert_eq!(by_key(&m)["a"], Change::Moved);
        assert_eq!(by_key(&m)["b"], Change::Moved);
        assert!(diff(Some(&before), Some(&after), ChildOrder::SortedByKey).is_empty());
    }

    #[test]
    fn reparent_is_a_move_with_parents() {
        let child = node("c", "item", "hc", "hc");
        let mut a1 = node("a", "item", "ha", "sa1");
        a1.children.push(child.clone());
        let mut b2 = node("b", "item", "hb", "sb2");
        b2.children.push(child);
        let before = doc(vec![a1, node("b", "item", "hb", "hb")], "r1");
        let after = doc(vec![node("a", "item", "ha", "ha"), b2], "r2");
        let changes = diff(Some(&before), Some(&after), ChildOrder::SortedByKey);
        let c = changes.iter().find(|e| e.key == "c").unwrap();
        assert_eq!(c.change, Change::Moved);
        assert!(!c.content_changed);
        assert_eq!(c.from_parent.as_deref(), Some("a"));
        assert_eq!(c.to_parent.as_deref(), Some("b"));
        let m = by_key(&changes);
        assert_eq!(m["a"], Change::ModifiedDescendants);
        assert_eq!(m["b"], Change::ModifiedDescendants);
    }

    #[test]
    fn moved_and_edited_flags_content() {
        let mut a1 = node("a", "item", "ha", "sa1");
        a1.children.push(node("c", "item", "hc1", "hc1"));
        let mut b2 = node("b", "item", "hb", "sb2");
        b2.children.push(node("c", "item", "hc2", "hc2"));
        let before = doc(vec![a1, node("b", "item", "hb", "hb")], "r1");
        let after = doc(vec![node("a", "item", "ha", "ha"), b2], "r2");
        let changes = diff(Some(&before), Some(&after), ChildOrder::SortedByKey);
        let c = changes.iter().find(|e| e.key == "c").unwrap();
        assert_eq!(c.change, Change::Moved);
        assert!(c.content_changed);
    }

    #[test]
    fn compact_collapses_added_subtrees() {
        let setup = heading("intro/setup", "setup-s1", None, vec![]);
        let after = doc(vec![heading("intro", "intro-s1", None, vec![setup])], "r1");
        let raw = diff(None, Some(&after), O);
        let c = compact(&raw, None, Some(&after));
        assert_eq!(
            c.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            ["intro"]
        );
        assert!(c.iter().all(|e| e.change == Change::Added));
    }

    #[test]
    fn compact_keeps_sibling_adds_and_modifications() {
        let before = doc(vec![heading("intro", "s1", None, vec![])], "r1");
        let after = doc(
            vec![
                heading("intro", "s1-CHANGED", Some("p2"), vec![]),
                heading("usage", "s2", None, vec![]),
            ],
            "r2",
        );
        let raw = diff(Some(&before), Some(&after), O);
        let c = compact(&raw, Some(&before), Some(&after));
        let m = by_key(&c);
        assert_eq!(m["intro#prose"], Change::Modified);
        assert_eq!(m["usage"], Change::Added);
        assert_eq!(m.get("intro"), None);
        assert_eq!(m.get("usage#prose"), None);
    }

    #[test]
    fn compact_collapses_removed_subtrees() {
        let before = doc(
            vec![
                heading("intro", "s1", None, vec![]),
                heading("usage", "s2", None, vec![]),
            ],
            "r1",
        );
        let after = doc(vec![heading("intro", "s1", None, vec![])], "r2");
        let raw = diff(Some(&before), Some(&after), O);
        let c = compact(&raw, Some(&before), Some(&after));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].key, "usage");
        assert_eq!(c[0].change, Change::Removed);
    }

    #[test]
    fn entry_json_matches_ts_shape() {
        let e = entry(&node("k", "heading", "h", "h"), Change::ModifiedDescendants);
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"key": "k", "kind": "heading", "change": "modified-descendants", "label": "k"})
        );
    }
}
