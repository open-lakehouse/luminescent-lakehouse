//! factory-core's types as factory.v1 wire messages: what factory-server's
//! RPCs answer with, and what `workgraph-mcp`'s plan review hands its View, so
//! both describe a revision the same way.

use factory_proto::v1;
use plan_tree::{Change, DiffEntry, MerkleNode};
use serde_json::Value;

use crate::GraphSummary;
use crate::backend::StoredRevision;

pub fn timestamp(t: chrono::DateTime<chrono::Utc>) -> buffa_types::google::protobuf::Timestamp {
    buffa_types::google::protobuf::Timestamp::from_unix(
        t.timestamp(),
        t.timestamp_subsec_nanos() as i32,
    )
}

pub fn summary(s: GraphSummary) -> v1::GraphSummary {
    v1::GraphSummary {
        root: s.root.into(),
        node_count: s.node_count as u32,
        revision: s.revision,
        approved_revision: s.approved.unwrap_or_default(),
        drifted_count: s.drifted_count as u32,
        ..Default::default()
    }
}

pub fn revision(r: &StoredRevision, approved: Option<&str>) -> v1::Revision {
    v1::Revision {
        root_hash: r.root_hash.clone(),
        parent_root_hash: r.parent_root_hash.clone(),
        sequence: r.sequence,
        provenance: r.provenance.to_proto().into(),
        created_at: timestamp(r.created_at).into(),
        changes: r.changes.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        approved: approved == Some(r.root_hash.as_str()),
        ..Default::default()
    }
}

pub fn tree(n: &MerkleNode) -> v1::MerkleNode {
    v1::MerkleNode {
        key: n.key.clone(),
        kind: n.kind.clone(),
        node_hash: n.node_hash.clone(),
        subtree_hash: n.subtree_hash.clone(),
        label: n.label.clone(),
        children: n.children.iter().map(tree).collect(),
        attrs: n
            .attrs
            .iter()
            .map(|(k, v)| {
                let s = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                (k.clone(), s)
            })
            .collect(),
        ..Default::default()
    }
}

pub fn change(c: Change) -> v1::ChangeKind {
    match c {
        Change::Added => v1::ChangeKind::CHANGE_KIND_ADDED,
        Change::Removed => v1::ChangeKind::CHANGE_KIND_REMOVED,
        Change::Modified => v1::ChangeKind::CHANGE_KIND_MODIFIED,
        Change::ModifiedDescendants => v1::ChangeKind::CHANGE_KIND_MODIFIED_DESCENDANTS,
        Change::Moved => v1::ChangeKind::CHANGE_KIND_MOVED,
    }
}

pub fn diff_entry(e: DiffEntry) -> v1::DiffEntry {
    v1::DiffEntry {
        key: e.key,
        kind: e.kind,
        change: change(e.change).into(),
        label: e.label,
        content_changed: e.content_changed,
        from_parent: e.from_parent.unwrap_or_default(),
        to_parent: e.to_parent.unwrap_or_default(),
        ..Default::default()
    }
}

/// `GetSchema`'s answer: the merged schema and every extension's descriptors.
pub fn schema_response(extensions: &crate::Extensions) -> v1::GetSchemaResponse {
    v1::GetSchemaResponse {
        schema: extensions.schema().proto().clone().into(),
        file_descriptor_set: extensions.descriptors().to_vec(),
        ..Default::default()
    }
}

/// `GetGraph`'s answer for a graph's state.
pub fn graph_response(state: crate::GraphState) -> v1::GetGraphResponse {
    v1::GetGraphResponse {
        graph: state.graph.into(),
        revision: state.revision,
        approved_revision: state.approved.unwrap_or_default(),
        drifted: state.drifted,
        ..Default::default()
    }
}

/// `GetRevision`'s answer: the revision, its Merkle tree and its rendered text.
pub fn revision_response(rev: &StoredRevision, approved: Option<&str>) -> v1::GetRevisionResponse {
    v1::GetRevisionResponse {
        revision: revision(rev, approved).into(),
        tree: tree(&rev.tree).into(),
        text: rev.text.clone(),
        sections: rev
            .sections
            .iter()
            .map(|s| v1::TextSection {
                key: s.key.clone(),
                start_line: s.start_line,
                end_line: s.end_line,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}
