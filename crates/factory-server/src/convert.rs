//! factory-core's types to and from the factory.v1 wire types.

use connectrpc::ConnectError;
use factory_core::backend::StoredRevision;
use factory_core::{Error, GraphSummary};
use factory_proto::v1;
use plan_tree::{Change, DiffEntry, MerkleNode};
use serde_json::Value;

pub fn error(e: Error) -> ConnectError {
    match e {
        Error::NotFound(m) => ConnectError::not_found(m),
        Error::AlreadyExists(m) => ConnectError::already_exists(m),
        Error::InvalidArgument(m) => ConnectError::invalid_argument(m),
        Error::FailedPrecondition(m) => ConnectError::failed_precondition(m),
        Error::Conflict(m) => ConnectError::aborted(m),
        Error::Internal(m) => {
            tracing::error!(%m, "internal error");
            ConnectError::internal(m)
        }
    }
}

fn timestamp(t: chrono::DateTime<chrono::Utc>) -> buffa_types::google::protobuf::Timestamp {
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
