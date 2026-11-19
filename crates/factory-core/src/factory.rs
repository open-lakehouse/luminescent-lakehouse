//! [`Factory`]: every read and write of a work graph goes through it. It
//! checks writes against the schema, derives node conditions on read, records
//! a Merkle revision whenever plan content changes, and tells watchers.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use buffa_types::google::protobuf::Any;
use chrono::Utc;
use factory_proto::workgraph::v1 as wg;
use plan_tree::{Change, ChildOrder, DiffEntry, MerkleNode};
use tokio::sync::{Mutex, broadcast};

use crate::backend::{Backend, Event, NewNode, NodeUpdate, Provenance, StoredRevision};
use crate::canonical::{self, Indexed};
use crate::extension::Extensions;
use crate::registry::Registry;
use crate::schema::{self, Schema, allows};
use crate::{Error, Result};

/// A change to a graph, as watchers see it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphChange {
    pub root: String,
    /// The revision after the change.
    pub revision: String,
    /// Increases on every change, content or not.
    pub sequence: u64,
    /// The run, when the change is a run update, so a watcher can patch it in
    /// place instead of refetching.
    pub run: Option<wg::Run>,
    /// The attention item, when the change raised or resolved one.
    pub attention: Option<wg::Attention>,
}

/// A graph with its derived conditions and its revision state.
#[derive(Debug, Clone)]
pub struct GraphState {
    pub graph: wg::Graph,
    pub revision: String,
    pub approved: Option<String>,
    /// Keys of the nodes that changed (compact diff) since the approved
    /// revision.
    pub drifted: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GraphSummary {
    pub root: wg::Node,
    pub node_count: usize,
    pub revision: String,
    pub approved: Option<String>,
    pub drifted_count: usize,
}

/// The condition types the core derives.
pub mod condition {
    pub const READY: &str = "Ready";
    pub const BLOCKED: &str = "Blocked";
    pub const RUNNING: &str = "Running";
    pub const STALLED: &str = "Stalled";
    pub const DRIFTED: &str = "Drifted";
    /// A live run is in review (its stage is `in_review` or `fixing`).
    pub const IN_REVIEW: &str = "InReview";
    /// A live run waits on a gate or a merge (its stage is `awaiting_merge`
    /// or `awaiting_gate`).
    pub const AWAITING_GATE: &str = "AwaitingGate";
}

pub struct Factory {
    backend: Arc<dyn Backend>,
    extensions: Extensions,
    registry: Registry,
    changes: broadcast::Sender<GraphChange>,
    sequence: AtomicU64,
    writes: Mutex<()>,
}

fn resolved(phase: wg::Phase) -> bool {
    matches!(phase, wg::Phase::PHASE_DONE | wg::Phase::PHASE_DROPPED)
}

fn phase_of(node: &wg::Node) -> wg::Phase {
    node.status
        .as_option()
        .and_then(|s| s.phase.as_known())
        .unwrap_or(wg::Phase::PHASE_UNSPECIFIED)
}

fn cond(kind: &str, status: bool, reason: &str, message: String) -> wg::Condition {
    wg::Condition {
        r#type: kind.into(),
        status,
        reason: reason.into(),
        message,
        ..Default::default()
    }
}

fn change_name(c: Change) -> &'static str {
    match c {
        Change::Added => "added",
        Change::Removed => "removed",
        Change::Modified => "modified",
        Change::ModifiedDescendants => "modified-descendants",
        Change::Moved => "moved",
    }
}

impl Factory {
    /// A factory over `backend` with the merged `extensions`. The backend
    /// must have been opened with their schema. [`FactoryBuilder`]
    /// (crate::FactoryBuilder) is the usual way in.
    pub fn new(backend: Arc<dyn Backend>, extensions: Extensions) -> Self {
        factory_proto::register_types();
        let (changes, _) = broadcast::channel(256);
        Self {
            backend,
            extensions,
            registry: Registry::default(),
            changes,
            sequence: AtomicU64::new(0),
            writes: Mutex::new(()),
        }
    }

    /// Use `registry` for profiles and repos (empty by default).
    #[must_use]
    pub fn with_registry(mut self, registry: Registry) -> Self {
        self.registry = registry;
        self
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn schema(&self) -> &Schema {
        self.extensions.schema()
    }

    /// The merged extensions: registered conditions, hooks and descriptors.
    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.backend()
    }

    /// Changes to every graph, from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<GraphChange> {
        self.changes.subscribe()
    }

    /// The sequence number the next change will exceed.
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    fn notify(&self, root: &str, revision: &str) {
        self.send(root, revision, None, None);
    }

    fn send(
        &self,
        root: &str,
        revision: &str,
        run: Option<wg::Run>,
        attention: Option<wg::Attention>,
    ) {
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        // No receivers is fine.
        let _ = self.changes.send(GraphChange {
            root: root.into(),
            revision: revision.into(),
            sequence,
            run,
            attention,
        });
    }

    // --- reads ---------------------------------------------------------

    pub async fn list_graphs(&self) -> Result<Vec<GraphSummary>> {
        let mut out = Vec::new();
        for root in self.backend.roots().await? {
            let state = self.graph(&root.r#ref).await?;
            let root_node = state
                .graph
                .nodes
                .iter()
                .find(|n| n.r#ref == root.r#ref)
                .cloned()
                .unwrap_or(root);
            out.push(GraphSummary {
                root: root_node,
                node_count: state.graph.nodes.len(),
                revision: state.revision,
                approved: state.approved,
                drifted_count: state.drifted.len(),
            });
        }
        Ok(out)
    }

    /// The current tree of a graph, as a revision would record it.
    fn hash(&self, graph: &wg::Graph) -> Result<MerkleNode> {
        Ok(plan_tree::build(
            canonical::tree(graph, self.schema())?,
            ChildOrder::SortedByKey,
        )?)
    }

    pub async fn graph(&self, root: &str) -> Result<GraphState> {
        let mut graph = self.backend.graph(root).await?;
        let tree = self.hash(&graph)?;
        let approved = self.backend.approved(root).await?;
        let drifted = match &approved {
            Some(h) if *h != tree.subtree_hash => {
                let base = self.revision(root, Some(h)).await?.tree;
                let raw = plan_tree::diff(Some(&base), Some(&tree), ChildOrder::SortedByKey);
                plan_tree::compact(&raw, Some(&base), Some(&tree))
                    .into_iter()
                    .map(|e| e.key)
                    .collect()
            }
            _ => Vec::new(),
        };
        let runs = self.backend.runs(root).await?;
        self.derive_conditions(&mut graph, &drifted, &runs);
        Ok(GraphState {
            graph,
            revision: tree.subtree_hash,
            approved,
            drifted,
        })
    }

    fn derive_conditions(&self, graph: &mut wg::Graph, drifted: &[String], runs: &[wg::Run]) {
        let now = Utc::now().timestamp();
        let mut live: HashMap<&str, bool> = HashMap::new();
        let mut stages: HashMap<&str, &str> = HashMap::new();
        for run in runs {
            let state = run.state.as_known().unwrap_or_default();
            if matches!(
                state,
                wg::RunState::RUN_STATE_QUEUED
                    | wg::RunState::RUN_STATE_WORKING
                    | wg::RunState::RUN_STATE_INPUT_REQUIRED
            ) {
                let expired = run
                    .lease_expiry
                    .as_option()
                    .is_some_and(|t| t.seconds < now);
                live.insert(run.node.as_str(), !expired);
                stages.insert(run.node.as_str(), run.stage.as_str());
            }
        }
        let drifted: HashSet<&str> = drifted.iter().map(String::as_str).collect();
        let mut conditions: HashMap<String, Vec<wg::Condition>> = HashMap::new();
        {
            let idx = Indexed::new(graph);
            let phases: HashMap<&str, wg::Phase> =
                idx.nodes.iter().map(|(r, n)| (*r, phase_of(n))).collect();
            for node_ref in idx.nodes.keys() {
                let mut cs = Vec::new();
                let phase = phases[node_ref];
                let is_root = *node_ref == graph.root;

                let open: Vec<&str> = idx
                    .targets(node_ref, schema::BLOCKED_BY)
                    .iter()
                    .filter(|t| !phases.get(*t).copied().is_some_and(resolved))
                    .copied()
                    .collect();
                let title = |r: &str| {
                    idx.nodes
                        .get(r)
                        .map_or_else(|| r.to_owned(), |n| n.spec.title.clone())
                };
                if !open.is_empty() {
                    let names: Vec<String> = open.iter().map(|r| title(r)).collect();
                    cs.push(cond(
                        condition::BLOCKED,
                        true,
                        "OpenBlockers",
                        format!("waiting on {}", names.join(", ")),
                    ));
                }
                let running = live.get(node_ref).copied();
                if running == Some(true) {
                    cs.push(cond(
                        condition::RUNNING,
                        true,
                        "LiveRun",
                        "a run holds the lease".into(),
                    ));
                }
                if running == Some(true) {
                    match stages.get(node_ref).copied() {
                        Some("in_review" | "fixing") => cs.push(cond(
                            condition::IN_REVIEW,
                            true,
                            "RunInReview",
                            "the run's PRs are in review".into(),
                        )),
                        Some("awaiting_merge" | "awaiting_gate") => cs.push(cond(
                            condition::AWAITING_GATE,
                            true,
                            "RunAwaiting",
                            "the run waits on a merge or a gate".into(),
                        )),
                        _ => {}
                    }
                }
                if running == Some(false) {
                    cs.push(cond(
                        condition::STALLED,
                        true,
                        "LeaseExpired",
                        "the run's lease expired".into(),
                    ));
                }
                let self_drifted = drifted.contains(node_ref);
                if self_drifted {
                    cs.push(cond(
                        condition::DRIFTED,
                        true,
                        "ChangedSinceApproval",
                        "changed since the approved revision".into(),
                    ));
                }

                if !is_root && !resolved(phase) {
                    let ancestors = idx.ancestors(node_ref);
                    let unapproved = ancestors.iter().find(|a| {
                        !matches!(
                            phases.get(*a),
                            Some(wg::Phase::PHASE_APPROVED | wg::Phase::PHASE_DONE)
                        )
                    });
                    let drifted_ancestor = ancestors.iter().find(|a| drifted.contains(*a));
                    let open_children = idx
                        .children_of(node_ref)
                        .iter()
                        .any(|c| !phases.get(c).copied().is_some_and(resolved));
                    let (ready, reason, message) = if phase != wg::Phase::PHASE_APPROVED {
                        (false, "NotApproved", "the node isn't approved".to_owned())
                    } else if !open.is_empty() {
                        (false, "OpenBlockers", "a blocker is still open".to_owned())
                    } else if running.is_some() {
                        (false, "HasRun", "a run is already attached".to_owned())
                    } else if open_children {
                        (
                            false,
                            "OpenChildren",
                            "its children carry the work".to_owned(),
                        )
                    } else if let Some(a) = unapproved {
                        (
                            false,
                            "AncestorNotApproved",
                            format!("{} isn't approved", title(a)),
                        )
                    } else if self_drifted || drifted_ancestor.is_some() {
                        (
                            false,
                            "Drifted",
                            "changed since the approved revision".to_owned(),
                        )
                    } else {
                        (true, "Dispatchable", "can be dispatched".to_owned())
                    };
                    cs.insert(0, cond(condition::READY, ready, reason, message));
                }
                conditions.insert((*node_ref).to_owned(), cs);
            }
        }
        for node in &mut graph.nodes {
            if let Some(cs) = conditions.remove(&node.r#ref) {
                node.status.get_or_insert_default().conditions = cs;
            }
        }
    }

    pub async fn node(&self, node_ref: &str) -> Result<wg::Node> {
        self.backend.node(node_ref).await
    }

    pub async fn annotations(&self, node_ref: &str) -> Result<Vec<wg::Annotation>> {
        self.backend.node(node_ref).await?;
        self.backend.annotations(node_ref).await
    }

    pub async fn runs(&self, root: &str) -> Result<Vec<wg::Run>> {
        self.backend.runs(root).await
    }

    pub async fn events(&self, root: &str) -> Result<Vec<Event>> {
        self.backend.events(root).await
    }

    /// Newest first.
    pub async fn revisions(&self, root: &str) -> Result<Vec<StoredRevision>> {
        let mut revs = self.backend.revisions(root).await?;
        revs.reverse();
        Ok(revs)
    }

    /// A revision by root hash, or the latest.
    pub async fn revision(&self, root: &str, root_hash: Option<&str>) -> Result<StoredRevision> {
        let revs = self.backend.revisions(root).await?;
        match root_hash {
            None | Some("") => revs.into_iter().next_back(),
            // The latest revision with that hash: a plan can return to an
            // earlier state.
            Some(h) => revs.into_iter().rev().find(|r| r.root_hash == h),
        }
        .ok_or_else(|| Error::NotFound(format!("revision {root_hash:?} of {root}")))
    }

    /// Diff two revisions by root hash. A `None` base diffs against nothing; a
    /// `None` target is the latest revision.
    pub async fn diff(
        &self,
        root: &str,
        base: Option<&str>,
        target: Option<&str>,
        compact: bool,
    ) -> Result<Vec<DiffEntry>> {
        let target = self.revision(root, target).await?.tree;
        let base = match base {
            None | Some("") => None,
            Some(h) => Some(self.revision(root, Some(h)).await?.tree),
        };
        let raw = plan_tree::diff(base.as_ref(), Some(&target), ChildOrder::SortedByKey);
        Ok(if compact {
            plan_tree::compact(&raw, base.as_ref(), Some(&target))
        } else {
            raw
        })
    }

    // --- writes --------------------------------------------------------

    /// Record a revision if the graph's content changed since the last one.
    /// Returns the current root hash.
    async fn record(&self, root: &str, provenance: &Provenance) -> Result<String> {
        let graph = self.backend.graph(root).await?;
        let tree = self.hash(&graph)?;
        let revs = self.backend.revisions(root).await?;
        let last = revs.last();
        if last.is_some_and(|r| r.root_hash == tree.subtree_hash) {
            return Ok(tree.subtree_hash);
        }
        let mut changes = BTreeMap::new();
        let raw = plan_tree::diff(last.map(|r| &r.tree), Some(&tree), ChildOrder::SortedByKey);
        for e in plan_tree::compact(&raw, last.map(|r| &r.tree), Some(&tree)) {
            *changes.entry(change_name(e.change).to_owned()).or_insert(0) += 1;
        }
        let text = canonical::render(&graph, self.schema())?;
        let revision = StoredRevision {
            root_hash: tree.subtree_hash.clone(),
            parent_root_hash: last.map(|r| r.root_hash.clone()).unwrap_or_default(),
            sequence: last.map_or(1, |r| r.sequence + 1),
            provenance: provenance.clone(),
            created_at: Utc::now(),
            changes,
            tree,
            text: text.text,
            sections: text.sections,
        };
        self.backend.append_revision(root, &revision).await?;
        tracing::debug!(root, revision = %revision.root_hash, seq = revision.sequence, "revision");
        Ok(revision.root_hash)
    }

    async fn event(
        &self,
        root: &str,
        node: &str,
        kind: &str,
        detail: String,
        p: &Provenance,
    ) -> Result<()> {
        self.backend
            .append_event(
                root,
                &Event {
                    node: node.into(),
                    kind: kind.into(),
                    detail,
                    provenance: p.clone(),
                    at: Utc::now(),
                },
            )
            .await
    }

    /// Check a spec against its kind, filling in an empty payload of the kind
    /// when there is none.
    fn checked_spec(&self, kind: &str, mut spec: wg::NodeSpec) -> Result<wg::NodeSpec> {
        if self.schema().node_kind(kind).is_none() {
            return Err(Error::invalid(format!("unknown node kind {kind:?}")));
        }
        match spec.payload.as_option() {
            None => {
                spec.payload = Any {
                    type_url: kind.into(),
                    ..Default::default()
                }
                .into();
            }
            Some(any) if any.type_url != kind => {
                return Err(Error::invalid(format!(
                    "payload is {:?}, the node kind is {kind:?}",
                    any.type_url
                )));
            }
            Some(any) => self.extensions.validate_payload(kind, &any.value)?,
        }
        if spec.title.trim().is_empty() {
            return Err(Error::invalid("a node needs a title"));
        }
        Ok(spec)
    }

    pub async fn create_node(
        &self,
        parent: Option<&str>,
        kind: &str,
        spec: wg::NodeSpec,
        labels: HashMap<String, String>,
        provenance: &Provenance,
    ) -> Result<wg::Node> {
        let _w = self.writes.lock().await;
        let spec = self.checked_spec(kind, spec)?;
        let node_kind = self.schema().node_kind(kind).expect("checked");
        let root = match parent {
            None => {
                if !node_kind.root {
                    return Err(Error::invalid(format!("{kind} can't be a graph's root")));
                }
                None
            }
            Some(p) => {
                let parent_node = self.backend.node(p).await?;
                let pkind = self.schema().node_kind(&parent_node.kind).ok_or_else(|| {
                    Error::internal(format!("unregistered kind {}", parent_node.kind))
                })?;
                if !pkind.allowed_child_kinds.iter().any(|k| k == kind) {
                    return Err(Error::invalid(format!(
                        "a {} can't have a {} child",
                        schema::short_kind(&parent_node.kind),
                        schema::short_kind(kind)
                    )));
                }
                Some(self.backend.root_of(p).await?)
            }
        };
        let node = self
            .backend
            .create_node(
                parent,
                NewNode {
                    kind: kind.into(),
                    spec,
                    labels,
                    phase: wg::Phase::PHASE_PROPOSED,
                    created_by: provenance.actor.clone(),
                },
            )
            .await?;
        let root = root.unwrap_or_else(|| node.r#ref.clone());
        let rev = self.record(&root, provenance).await?;
        self.notify(&root, &rev);
        Ok(node)
    }

    pub async fn update_node(
        &self,
        node_ref: &str,
        spec: Option<wg::NodeSpec>,
        labels: Option<HashMap<String, String>>,
        etag: Option<String>,
        provenance: &Provenance,
    ) -> Result<wg::Node> {
        let _w = self.writes.lock().await;
        let current = self.backend.node(node_ref).await?;
        let spec = spec
            .map(|s| self.checked_spec(&current.kind, s))
            .transpose()?;
        let root = self.backend.root_of(node_ref).await?;
        let node = self
            .backend
            .update_node(
                node_ref,
                NodeUpdate {
                    spec,
                    labels,
                    phase: None,
                    etag: etag.filter(|e| !e.is_empty()),
                },
            )
            .await?;
        let rev = self.record(&root, provenance).await?;
        self.notify(&root, &rev);
        Ok(node)
    }

    pub async fn move_node(
        &self,
        node_ref: &str,
        new_parent: &str,
        provenance: &Provenance,
    ) -> Result<wg::Node> {
        let _w = self.writes.lock().await;
        let root = self.backend.root_of(node_ref).await?;
        if root == node_ref {
            return Err(Error::precondition("the root can't be moved"));
        }
        if self.backend.root_of(new_parent).await? != root {
            return Err(Error::precondition("a node can only move within its graph"));
        }
        let graph = self.backend.graph(&root).await?;
        let idx = Indexed::new(&graph);
        if new_parent == node_ref || idx.descendants(node_ref).contains(&new_parent) {
            return Err(Error::precondition("a node can't move below itself"));
        }
        let node = idx.nodes[node_ref];
        let parent_node = idx.nodes[new_parent];
        let pkind = self
            .schema()
            .node_kind(&parent_node.kind)
            .ok_or_else(|| Error::internal("unregistered parent kind"))?;
        if !pkind.allowed_child_kinds.contains(&node.kind) {
            return Err(Error::invalid(format!(
                "a {} can't have a {} child",
                schema::short_kind(&parent_node.kind),
                schema::short_kind(&node.kind)
            )));
        }
        let old_parent = idx
            .parent
            .get(node_ref)
            .copied()
            .ok_or_else(|| Error::internal("a non-root node without a parent"))?;
        if old_parent == new_parent {
            return Ok(node.clone());
        }
        let old_parent = old_parent.to_owned();
        self.backend
            .move_node(node_ref, &old_parent, new_parent)
            .await?;
        let rev = self.record(&root, provenance).await?;
        self.notify(&root, &rev);
        self.backend.node(node_ref).await
    }

    pub async fn delete_node(
        &self,
        node_ref: &str,
        recursive: bool,
        provenance: &Provenance,
    ) -> Result<()> {
        let _w = self.writes.lock().await;
        let root = self.backend.root_of(node_ref).await?;
        let graph = self.backend.graph(&root).await?;
        let idx = Indexed::new(&graph);
        let below = idx.descendants(node_ref);
        if !below.is_empty() && !recursive {
            return Err(Error::precondition(format!(
                "{node_ref} has {} node(s) below it; delete recursively",
                below.len()
            )));
        }
        let mut refs: Vec<String> = below.iter().map(|s| (*s).to_owned()).collect();
        refs.push(node_ref.to_owned());
        self.backend.delete_nodes(&refs).await?;
        if root == node_ref {
            self.notify(&root, "");
            return Ok(());
        }
        let rev = self.record(&root, provenance).await?;
        self.notify(&root, &rev);
        Ok(())
    }

    async fn check_edge(&self, edge: &wg::Edge) -> Result<(String, wg::Graph)> {
        let kind = self
            .schema()
            .edge_kind(&edge.kind)
            .ok_or_else(|| Error::invalid(format!("unknown edge kind {:?}", edge.kind)))?;
        if kind.topology == wg::Topology::TOPOLOGY_TREE {
            return Err(Error::invalid(format!(
                "{} edges are the hierarchy; create or move the node instead",
                edge.kind
            )));
        }
        if edge.from == edge.to {
            return Err(Error::invalid("an edge can't point at its own source"));
        }
        let root = self.backend.root_of(&edge.from).await?;
        if self.backend.root_of(&edge.to).await? != root {
            return Err(Error::invalid("an edge can't join two graphs"));
        }
        let graph = self.backend.graph(&root).await?;
        Ok((root, graph))
    }

    pub async fn add_edge(&self, edge: &wg::Edge, provenance: &Provenance) -> Result<()> {
        let _w = self.writes.lock().await;
        let (root, graph) = self.check_edge(edge).await?;
        let kind = self.schema().edge_kind(&edge.kind).expect("checked");
        let idx = Indexed::new(&graph);
        let (from, to) = (idx.nodes[edge.from.as_str()], idx.nodes[edge.to.as_str()]);
        if !allows(&kind.from_kinds, &from.kind) || !allows(&kind.to_kinds, &to.kind) {
            return Err(Error::invalid(format!(
                "{} can't join a {} to a {}",
                edge.kind,
                schema::short_kind(&from.kind),
                schema::short_kind(&to.kind)
            )));
        }
        if kind.topology == wg::Topology::TOPOLOGY_DAG {
            // A path from `to` back to `from` would close a cycle.
            let mut stack = vec![edge.to.as_str()];
            let mut seen = HashSet::new();
            while let Some(n) = stack.pop() {
                if n == edge.from {
                    return Err(Error::precondition(format!(
                        "{} would form a cycle",
                        edge.kind
                    )));
                }
                if seen.insert(n) {
                    stack.extend(idx.targets(n, &edge.kind).iter().copied());
                }
            }
        }
        self.backend.add_edge(edge).await?;
        let rev = self.record(&root, provenance).await?;
        self.notify(&root, &rev);
        Ok(())
    }

    pub async fn remove_edge(&self, edge: &wg::Edge, provenance: &Provenance) -> Result<()> {
        let _w = self.writes.lock().await;
        let (root, _) = self.check_edge(edge).await?;
        self.backend.remove_edge(edge).await?;
        let rev = self.record(&root, provenance).await?;
        self.notify(&root, &rev);
        Ok(())
    }

    pub async fn set_phase(
        &self,
        node_ref: &str,
        phase: wg::Phase,
        provenance: &Provenance,
    ) -> Result<wg::Node> {
        if phase == wg::Phase::PHASE_UNSPECIFIED {
            return Err(Error::invalid("a phase is required"));
        }
        let _w = self.writes.lock().await;
        let root = self.backend.root_of(node_ref).await?;
        let node = self
            .backend
            .update_node(
                node_ref,
                NodeUpdate {
                    phase: Some(phase),
                    ..Default::default()
                },
            )
            .await?;
        self.event(&root, node_ref, "phase", format!("{phase:?}"), provenance)
            .await?;
        let rev = self.record(&root, provenance).await?;
        self.notify(&root, &rev);
        Ok(node)
    }

    pub async fn annotate(
        &self,
        node_ref: &str,
        kind: &str,
        payload: Option<Any>,
        provenance: &Provenance,
    ) -> Result<wg::Annotation> {
        if kind.is_empty() {
            return Err(Error::invalid("an annotation needs a kind"));
        }
        if let Some(p) = &payload
            && p.type_url != kind
        {
            return Err(Error::invalid(
                "the payload's type isn't the annotation's kind",
            ));
        }
        let _w = self.writes.lock().await;
        let root = self.backend.root_of(node_ref).await?;
        let annotation = wg::Annotation {
            r#ref: crate::olai::new_ref(),
            node: node_ref.into(),
            kind: kind.into(),
            payload: payload.into(),
            provenance: provenance.to_proto().into(),
            created_at: buffa_types::google::protobuf::Timestamp::now().into(),
            ..Default::default()
        };
        self.backend.add_annotation(&annotation).await?;
        self.event(
            &root,
            node_ref,
            "annotation",
            schema::short_kind(kind).into(),
            provenance,
        )
        .await?;
        let rev = self.record(&root, provenance).await?;
        self.notify(&root, &rev);
        Ok(annotation)
    }

    /// Record a run (the reconciler's job; also seeds demos).
    pub async fn put_run(&self, run: &wg::Run, provenance: &Provenance) -> Result<()> {
        let _w = self.writes.lock().await;
        let root = self.backend.root_of(&run.node).await?;
        self.backend.put_run(&root, run).await?;
        self.event(
            &root,
            &run.node,
            "run",
            format!("{:?}", run.state),
            provenance,
        )
        .await?;
        let rev = self.record(&root, provenance).await?;
        self.send(&root, &rev, Some(run.clone()), None);
        Ok(())
    }

    /// The ref of the graph root `node_ref` belongs to.
    pub async fn root_of(&self, node_ref: &str) -> Result<String> {
        self.backend.root_of(node_ref).await
    }

    /// A run by its ref alone, with its graph's root.
    pub async fn find_run(&self, run_ref: &str) -> Result<(String, wg::Run)> {
        for root in self.backend.roots().await? {
            if let Some(run) = self
                .backend
                .runs(&root.r#ref)
                .await?
                .into_iter()
                .find(|r| r.r#ref == run_ref)
            {
                return Ok((root.r#ref, run));
            }
        }
        Err(Error::NotFound(format!("run {run_ref}")))
    }

    /// Add reported artifacts to a run: PRs (a PR group, in order) merged
    /// by URL with what Omnigent and the code host said, and releases.
    pub async fn report_artifacts(
        &self,
        run_ref: &str,
        artifacts: &[wg::Artifact],
        provenance: &Provenance,
    ) -> Result<wg::Run> {
        let (_, mut run) = self.find_run(run_ref).await?;
        let step = run.steps.last().map_or(0, |s| s.index);
        let mut prs = Vec::new();
        let mut releases = Vec::new();
        for a in artifacts {
            match &a.artifact {
                Some(wg::artifact::Artifact::PullRequest(pr)) if !pr.url.is_empty() => {
                    prs.push((**pr).clone());
                }
                Some(wg::artifact::Artifact::Release(r)) => releases.push((**r).clone()),
                _ => {
                    return Err(Error::invalid(
                        "an artifact needs a PR with a url, or a release",
                    ));
                }
            }
        }
        crate::artifact::merge_prs(
            &mut run,
            &prs,
            wg::ArtifactSource::ARTIFACT_SOURCE_REPORTED,
            step,
        );
        crate::artifact::merge_releases(&mut run, &releases, step);
        run.updated_at = buffa_types::google::protobuf::Timestamp::now().into();
        self.put_run(&run, provenance).await?;
        Ok(run)
    }

    /// Take or renew a run's lease for `holder` until `until`. It's taken
    /// when the run has no holder, `holder` has it, or the lease expired.
    /// Returns the run as leased, or `None` when another holder has it.
    ///
    /// The check is atomic within this process (every write holds the
    /// write lock). Across processes it isn't yet: run one reconciler per
    /// store.
    pub async fn lease_run(
        &self,
        root: &str,
        run_ref: &str,
        holder: &str,
        until: chrono::DateTime<Utc>,
    ) -> Result<Option<wg::Run>> {
        let _w = self.writes.lock().await;
        let mut run = self
            .backend
            .runs(root)
            .await?
            .into_iter()
            .find(|r| r.r#ref == run_ref)
            .ok_or_else(|| Error::NotFound(format!("run {run_ref}")))?;
        let expired = run
            .lease_expiry
            .as_option()
            .is_none_or(|t| t.seconds < Utc::now().timestamp());
        if !run.lease_holder.is_empty() && run.lease_holder != holder && !expired {
            return Ok(None);
        }
        let taken = run.lease_holder != holder;
        run.lease_holder = holder.into();
        run.lease_expiry = buffa_types::google::protobuf::Timestamp {
            seconds: until.timestamp(),
            ..Default::default()
        }
        .into();
        self.backend.put_run(root, &run).await?;
        if taken {
            // A renewal isn't news; a new holder is.
            let revision = self
                .record(root, &Provenance::new(holder, "reconciler"))
                .await?;
            self.send(root, &revision, Some(run.clone()), None);
        }
        Ok(Some(run))
    }

    /// Raise an attention item (a ref and creation time are filled in).
    pub async fn raise_attention(
        &self,
        mut item: wg::Attention,
        provenance: &Provenance,
    ) -> Result<wg::Attention> {
        let _w = self.writes.lock().await;
        let root = self.backend.root_of(&item.node).await?;
        if item.r#ref.is_empty() {
            item.r#ref = crate::olai::new_ref();
        }
        if item.created_at.as_option().is_none() {
            item.created_at = buffa_types::google::protobuf::Timestamp::now().into();
        }
        self.backend.put_attention(&root, &item).await?;
        self.event(
            &root,
            &item.node,
            "attention",
            item.summary.clone(),
            provenance,
        )
        .await?;
        let revision = self.record(&root, provenance).await?;
        self.send(&root, &revision, None, Some(item.clone()));
        Ok(item)
    }

    /// Every attention item of the graph under `root`, or of every graph,
    /// oldest first; resolved ones too with `include_resolved`.
    pub async fn attention(
        &self,
        root: Option<&str>,
        include_resolved: bool,
    ) -> Result<Vec<wg::Attention>> {
        let roots = match root {
            Some(r) => vec![r.to_owned()],
            None => self
                .backend
                .roots()
                .await?
                .into_iter()
                .map(|n| n.r#ref)
                .collect(),
        };
        let mut out = Vec::new();
        for root in roots {
            out.extend(
                self.backend
                    .attention(&root)
                    .await?
                    .into_iter()
                    .filter(|a| include_resolved || !a.resolved),
            );
        }
        Ok(out)
    }

    /// Resolve an attention item: `resolution` is `accept`, `decline`,
    /// `retry` or `dismiss`. The reconciler then acts on it and marks it
    /// applied ([`Self::update_attention`]).
    pub async fn resolve_attention(
        &self,
        item_ref: &str,
        resolution: &str,
        comment: &str,
        provenance: &Provenance,
    ) -> Result<wg::Attention> {
        let (root, mut item) = self.find_attention(item_ref).await?;
        if item.resolved {
            return Err(Error::precondition(format!(
                "{item_ref} is already resolved"
            )));
        }
        item.resolved = true;
        item.resolution = resolution.into();
        item.comment = comment.into();
        item.resolved_by = provenance.actor.clone();
        item.resolved_at = buffa_types::google::protobuf::Timestamp::now().into();
        self.update_attention(&root, &item, provenance).await?;
        Ok(item)
    }

    /// Replace an attention item.
    pub async fn update_attention(
        &self,
        root: &str,
        item: &wg::Attention,
        provenance: &Provenance,
    ) -> Result<()> {
        let _w = self.writes.lock().await;
        self.backend.put_attention(root, item).await?;
        let revision = self.record(root, provenance).await?;
        self.send(root, &revision, None, Some(item.clone()));
        Ok(())
    }

    async fn find_attention(&self, item_ref: &str) -> Result<(String, wg::Attention)> {
        for root in self.backend.roots().await? {
            if let Some(item) = self
                .backend
                .attention(&root.r#ref)
                .await?
                .into_iter()
                .find(|a| a.r#ref == item_ref)
            {
                return Ok((root.r#ref, item));
            }
        }
        Err(Error::NotFound(format!("attention {item_ref}")))
    }

    /// Record an event on a graph (an execution, an amendment).
    pub async fn append_event(
        &self,
        root: &str,
        node: &str,
        kind: &str,
        detail: String,
        provenance: &Provenance,
    ) -> Result<()> {
        let _w = self.writes.lock().await;
        self.event(root, node, kind, detail, provenance).await?;
        let revision = self.record(root, provenance).await?;
        self.notify(root, &revision);
        Ok(())
    }

    /// Where each session of the graph under `root` would run under
    /// `choice`, with `run` overriding one node's environment (see
    /// [`crate::placement`]).
    pub async fn placements(
        &self,
        root: &str,
        choice: &wg::PlacementChoice,
        run: Option<(&str, &str)>,
    ) -> Result<Vec<wg::NodePlacement>> {
        let graph = self.backend.graph(root).await?;
        for node in choice.nodes.keys() {
            if !graph.nodes.iter().any(|n| &n.r#ref == node) {
                return Err(Error::invalid(format!(
                    "the placement choice names {node}, which isn't in the graph"
                )));
            }
        }
        Ok(crate::placement::resolve(
            &self.registry,
            &graph,
            choice,
            run,
        ))
    }

    /// One run of the graph under `root`.
    pub async fn run(&self, root: &str, run_ref: &str) -> Result<wg::Run> {
        self.backend
            .runs(root)
            .await?
            .into_iter()
            .find(|r| r.r#ref == run_ref)
            .ok_or_else(|| Error::NotFound(format!("run {run_ref}")))
    }

    /// Approve the current revision. `root_hash` must be it, so what's
    /// approved is what was reviewed. Proposed nodes become approved.
    pub async fn approve(
        &self,
        root: &str,
        root_hash: &str,
        provenance: &Provenance,
    ) -> Result<StoredRevision> {
        let _w = self.writes.lock().await;
        let current = self.record(root, provenance).await?;
        if current != root_hash {
            return Err(Error::precondition(format!(
                "{root_hash} isn't the current revision ({current})"
            )));
        }
        let graph = self.backend.graph(root).await?;
        for node in &graph.nodes {
            if phase_of(node) == wg::Phase::PHASE_PROPOSED {
                self.backend
                    .update_node(
                        &node.r#ref,
                        NodeUpdate {
                            phase: Some(wg::Phase::PHASE_APPROVED),
                            ..Default::default()
                        },
                    )
                    .await?;
            }
        }
        self.backend.set_approved(root, root_hash).await?;
        self.event(root, root, "approve", root_hash.into(), provenance)
            .await?;
        self.notify(root, root_hash);
        self.revision(root, Some(root_hash)).await
    }
}
