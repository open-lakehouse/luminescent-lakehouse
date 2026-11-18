//! The seam: what a work-graph backend stores, and the records kept next to
//! it. Backends store and load; they don't validate. Schema checks, derived
//! conditions and revisions live in [`Factory`](crate::Factory), above every
//! backend, so all backends behave the same.

use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use factory_proto::workgraph::v1 as wg;
use plan_tree::MerkleNode;
use serde::{Deserialize, Serialize};

use crate::Result;

/// The read side of a work graph.
#[async_trait]
pub trait WorkGraphReader: Send + Sync + 'static {
    /// The backend's name, as nodes report it in `metadata.backend`.
    fn backend(&self) -> &'static str;
    /// The root node of every graph.
    async fn roots(&self) -> Result<Vec<wg::Node>>;
    /// Every node and edge of the graph rooted at `root`, flat.
    async fn graph(&self, root: &str) -> Result<wg::Graph>;
    async fn node(&self, node_ref: &str) -> Result<wg::Node>;
    /// The ref of the root of the graph `node_ref` belongs to.
    async fn root_of(&self, node_ref: &str) -> Result<String>;
}

/// A node to create.
#[derive(Debug, Clone, Default)]
pub struct NewNode {
    pub kind: String,
    pub spec: wg::NodeSpec,
    pub labels: HashMap<String, String>,
    pub phase: wg::Phase,
    pub created_by: String,
}

/// The fields of a node an update replaces; `None` keeps the current value.
#[derive(Debug, Clone, Default)]
pub struct NodeUpdate {
    pub spec: Option<wg::NodeSpec>,
    pub labels: Option<HashMap<String, String>>,
    pub phase: Option<wg::Phase>,
    /// Refuse the update with `Conflict` unless the node's etag is this.
    pub etag: Option<String>,
}

/// The write side of a work graph.
#[async_trait]
pub trait WorkGraph: WorkGraphReader {
    /// Create a node: under `parent` (with its `child_of` edge), or as the
    /// root of a new graph.
    async fn create_node(&self, parent: Option<&str>, node: NewNode) -> Result<wg::Node>;
    async fn update_node(&self, node_ref: &str, update: NodeUpdate) -> Result<wg::Node>;
    /// Swap a node's `child_of` edge from one parent to another, atomically.
    async fn move_node(&self, node_ref: &str, from_parent: &str, to_parent: &str) -> Result<()>;
    /// Delete nodes and every edge that touches them.
    async fn delete_nodes(&self, refs: &[String]) -> Result<()>;
    async fn add_edge(&self, edge: &wg::Edge) -> Result<()>;
    async fn remove_edge(&self, edge: &wg::Edge) -> Result<()>;
}

/// A recorded state of a graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredRevision {
    pub root_hash: String,
    #[serde(default)]
    pub parent_root_hash: String,
    pub sequence: u64,
    pub provenance: Provenance,
    pub created_at: DateTime<Utc>,
    /// Compact change counts against the parent, by change kind.
    #[serde(default)]
    pub changes: BTreeMap<String, u32>,
    pub tree: MerkleNode,
    pub text: String,
    pub sections: Vec<TextSection>,
}

/// Who made a change, and through which path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub actor: String,
    pub via: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run_ref: String,
}

impl Provenance {
    pub fn new(actor: impl Into<String>, via: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            via: via.into(),
            run_ref: String::new(),
        }
    }

    pub fn to_proto(&self) -> wg::Provenance {
        wg::Provenance {
            actor: self.actor.clone(),
            via: self.via.clone(),
            run_ref: self.run_ref.clone(),
            ..Default::default()
        }
    }
}

/// Where a node's section sits in a revision's rendered text, 1-based and
/// inclusive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSection {
    pub key: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// A change that isn't plan content: a phase, an annotation, an approval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub node: String,
    /// `phase`, `annotation`, `approve`, `run`.
    pub kind: String,
    pub detail: String,
    pub provenance: Provenance,
    pub at: DateTime<Utc>,
}

/// The records kept next to the graph. Never hashed.
#[async_trait]
pub trait RecordStore: Send + Sync + 'static {
    async fn append_revision(&self, root: &str, revision: &StoredRevision) -> Result<()>;
    /// Oldest first.
    async fn revisions(&self, root: &str) -> Result<Vec<StoredRevision>>;
    async fn set_approved(&self, root: &str, root_hash: &str) -> Result<()>;
    async fn approved(&self, root: &str) -> Result<Option<String>>;
    async fn add_annotation(&self, annotation: &wg::Annotation) -> Result<()>;
    /// Oldest first.
    async fn annotations(&self, node_ref: &str) -> Result<Vec<wg::Annotation>>;
    /// Create or replace a run.
    async fn put_run(&self, root: &str, run: &wg::Run) -> Result<()>;
    async fn runs(&self, root: &str) -> Result<Vec<wg::Run>>;
    async fn append_event(&self, root: &str, event: &Event) -> Result<()>;
    /// Oldest first.
    async fn events(&self, root: &str) -> Result<Vec<Event>>;
}

/// A backend that holds both the graph and its records.
pub trait Backend: WorkGraph + RecordStore {}

impl<T: WorkGraph + RecordStore> Backend for T {}
