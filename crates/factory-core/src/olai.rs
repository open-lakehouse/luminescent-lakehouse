//! The olai-store backend: the work graph as olai-store objects and
//! associations, with its records (revisions, annotations, runs, events,
//! attention) in
//! the same store.
//!
//! - A node is an object labeled `node`, named flat `[graph, node]`: names
//!   carry graph membership and never the hierarchy. Its properties are
//!   `{kind, spec, phase, labels, createdBy}`, with `spec` as proto-JSON.
//! - An edge is an association labeled with its kind's name (`child_of`, …),
//!   stored in one direction only.
//! - The etag is the object's version, so updates are compare-and-swap.
//!
//! Refs are `olai:<uuid>`.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use factory_proto::workgraph::v1 as wg;
use olai_store::{
    AssociationStore, EdgeQuery, Label, Object, ObjectStore, Precondition, ResourceName, StoreExec,
    Transactional,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::backend::{
    Event, NewNode, NodeUpdate, RecordStore, StoredRevision, WorkGraph, WorkGraphReader,
};
use crate::schema::Schema;
use crate::{Error, Result};

/// The object labels the factory stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactoryLabel {
    Node,
    Annotation,
    Revision,
    Run,
    Approval,
    Event,
    Attention,
}

impl FactoryLabel {
    const ALL: [FactoryLabel; 7] = [
        Self::Node,
        Self::Annotation,
        Self::Revision,
        Self::Run,
        Self::Approval,
        Self::Event,
        Self::Attention,
    ];
}

impl std::fmt::Display for FactoryLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for FactoryLabel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|l| l.as_str() == s)
            .ok_or_else(|| format!("unknown label: {s}"))
    }
}

impl Label for FactoryLabel {
    fn as_str(&self) -> &str {
        match self {
            Self::Node => "node",
            Self::Annotation => "annotation",
            Self::Revision => "revision",
            Self::Run => "run",
            Self::Approval => "approval",
            Self::Event => "event",
            Self::Attention => "attention",
        }
    }
}

const PREFIX: &str = "olai:";

fn to_ref(id: Uuid) -> String {
    format!("{PREFIX}{id}")
}

fn parse_ref(node_ref: &str) -> Result<Uuid> {
    node_ref
        .strip_prefix(PREFIX)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| Error::invalid(format!("not an olai ref: {node_ref:?}")))
}

fn timestamp(t: DateTime<Utc>) -> buffa_types::google::protobuf::Timestamp {
    buffa_types::google::protobuf::Timestamp::from_unix(
        t.timestamp(),
        t.timestamp_subsec_nanos() as i32,
    )
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeProps {
    kind: String,
    spec: Value,
    phase: buffa::EnumValue<wg::Phase>,
    #[serde(default)]
    labels: HashMap<String, String>,
    #[serde(default)]
    created_by: String,
}

/// The work graph and its records on an olai-store.
pub struct OlaiBackend<S> {
    store: S,
    /// The edge kinds to load, from the schema.
    edge_kinds: Vec<String>,
}

impl<S> OlaiBackend<S>
where
    S: ObjectStore<FactoryLabel> + AssociationStore<FactoryLabel> + Transactional<FactoryLabel>,
{
    pub fn new(store: S, schema: &Schema) -> Self {
        factory_proto::register_types();
        Self {
            store,
            edge_kinds: schema.edge_kinds().map(|k| k.name.clone()).collect(),
        }
    }

    async fn list_all(
        &self,
        label: FactoryLabel,
        namespace: Option<&ResourceName>,
    ) -> Result<Vec<Object<FactoryLabel>>> {
        let mut out = Vec::new();
        let mut token = None;
        loop {
            let (page, next) = self.store.list(label, namespace, None, token).await?;
            out.extend(page);
            match next {
                Some(t) => token = Some(t),
                None => return Ok(out),
            }
        }
    }

    fn to_node(&self, obj: &Object<FactoryLabel>) -> Result<wg::Node> {
        let props: NodeProps = serde_json::from_value(
            obj.properties
                .clone()
                .ok_or_else(|| Error::internal("node without properties"))?,
        )?;
        let spec: wg::NodeSpec = serde_json::from_value(props.spec)?;
        Ok(wg::Node {
            r#ref: to_ref(obj.id),
            kind: props.kind,
            spec: spec.into(),
            status: wg::NodeStatus {
                phase: props.phase,
                ..Default::default()
            }
            .into(),
            metadata: wg::NodeMetadata {
                labels: props.labels,
                backend: self.backend().into(),
                etag: obj.version.to_string(),
                created_by: props.created_by,
                created_at: timestamp(obj.created_at).into(),
                updated_at: obj.updated_at.map(timestamp).into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        })
    }

    fn graph_namespace(obj: &Object<FactoryLabel>) -> Result<Uuid> {
        let seg = obj
            .name
            .path()
            .first()
            .ok_or_else(|| Error::internal("node without a graph"))?;
        Uuid::parse_str(seg).map_err(Error::internal)
    }

    async fn node_obj(&self, node_ref: &str) -> Result<Object<FactoryLabel>> {
        let id = parse_ref(node_ref)?;
        match self.store.get(&id).await {
            Ok(obj) if obj.label == FactoryLabel::Node => Ok(obj),
            Ok(_) | Err(olai_store::Error::NotFound) => Err(Error::NotFound(node_ref.into())),
            Err(e) => Err(e.into()),
        }
    }

    async fn put_record<T: Serialize>(
        &self,
        label: FactoryLabel,
        name: ResourceName,
        value: &T,
    ) -> Result<()> {
        let props = serde_json::to_value(value)?;
        match self.store.get_by_name(label, &name).await {
            Ok(obj) => {
                self.store
                    .update(&obj.id, Some(props), Precondition::Any, None)
                    .await?;
            }
            Err(olai_store::Error::NotFound) => {
                self.store
                    .create(label, &name, Some(props), None, None)
                    .await?;
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    async fn records<T: for<'de> Deserialize<'de>>(
        &self,
        label: FactoryLabel,
        namespace: &[String],
    ) -> Result<Vec<T>> {
        let ns = ResourceName::new(namespace.iter().cloned());
        self.list_all(label, Some(&ns))
            .await?
            .into_iter()
            .filter_map(|o| o.properties)
            .map(|p| serde_json::from_value(p).map_err(Error::from))
            .collect()
    }
}

async fn edges_from(
    store: &dyn StoreExec<FactoryLabel>,
    id: Uuid,
    kind: &str,
) -> Result<Vec<olai_store::Association<FactoryLabel>>> {
    let mut out = Vec::new();
    let mut token = None;
    loop {
        let mut q = EdgeQuery::from(id, kind);
        q.page_token = token;
        let (page, next) = store.query_edges(q).await?;
        out.extend(page);
        match next {
            Some(t) => token = Some(t),
            None => return Ok(out),
        }
    }
}

async fn edges_into(
    store: &dyn StoreExec<FactoryLabel>,
    id: Uuid,
    kind: &str,
) -> Result<Vec<olai_store::Association<FactoryLabel>>> {
    let mut out = Vec::new();
    let mut token = None;
    loop {
        let mut q = EdgeQuery::into(id, kind);
        q.page_token = token;
        let (page, next) = store.query_edges(q).await?;
        out.extend(page);
        match next {
            Some(t) => token = Some(t),
            None => return Ok(out),
        }
    }
}

#[async_trait]
impl<S> WorkGraphReader for OlaiBackend<S>
where
    S: ObjectStore<FactoryLabel> + AssociationStore<FactoryLabel> + Transactional<FactoryLabel>,
{
    fn backend(&self) -> &'static str {
        "olai"
    }

    async fn roots(&self) -> Result<Vec<wg::Node>> {
        self.list_all(FactoryLabel::Node, None)
            .await?
            .iter()
            .filter(|o| {
                let p = o.name.path();
                p.len() == 2 && p[0] == p[1]
            })
            .map(|o| self.to_node(o))
            .collect()
    }

    async fn graph(&self, root: &str) -> Result<wg::Graph> {
        let root_id = parse_ref(root)?;
        let ns = ResourceName::new([root_id.to_string()]);
        let objs = self.list_all(FactoryLabel::Node, Some(&ns)).await?;
        if !objs.iter().any(|o| o.id == root_id) {
            return Err(Error::NotFound(root.into()));
        }
        let mut edges = Vec::new();
        for obj in &objs {
            for kind in &self.edge_kinds {
                for a in edges_from(&self.store, obj.id, kind).await? {
                    edges.push(wg::Edge {
                        kind: a.label.clone(),
                        from: to_ref(a.from_id),
                        to: to_ref(a.to_id),
                        ..Default::default()
                    });
                }
            }
        }
        edges.sort_by(|a, b| (&a.kind, &a.from, &a.to).cmp(&(&b.kind, &b.from, &b.to)));
        Ok(wg::Graph {
            root: root.into(),
            nodes: objs
                .iter()
                .map(|o| self.to_node(o))
                .collect::<Result<_>>()?,
            edges,
            ..Default::default()
        })
    }

    async fn node(&self, node_ref: &str) -> Result<wg::Node> {
        self.to_node(&self.node_obj(node_ref).await?)
    }

    async fn root_of(&self, node_ref: &str) -> Result<String> {
        let obj = self.node_obj(node_ref).await?;
        Ok(to_ref(Self::graph_namespace(&obj)?))
    }
}

#[async_trait]
impl<S> WorkGraph for OlaiBackend<S>
where
    S: ObjectStore<FactoryLabel> + AssociationStore<FactoryLabel> + Transactional<FactoryLabel>,
{
    async fn create_node(&self, parent: Option<&str>, node: NewNode) -> Result<wg::Node> {
        let id = Uuid::now_v7();
        let (graph, parent_id) = match parent {
            None => (id, None),
            Some(p) => {
                let pobj = self.node_obj(p).await?;
                (Self::graph_namespace(&pobj)?, Some(pobj.id))
            }
        };
        let props = NodeProps {
            kind: node.kind,
            spec: serde_json::to_value(&node.spec)?,
            phase: node.phase.into(),
            labels: node.labels,
            created_by: node.created_by,
        };
        let name = ResourceName::new([graph.to_string(), id.to_string()]);
        let tx = self.store.begin().await?;
        let obj = tx
            .create(
                FactoryLabel::Node,
                &name,
                Some(serde_json::to_value(&props)?),
                Some(id),
                None,
            )
            .await?;
        if let Some(pid) = parent_id {
            tx.add(id, pid, crate::schema::CHILD_OF, None).await?;
        }
        tx.commit().await?;
        self.to_node(&obj)
    }

    async fn update_node(&self, node_ref: &str, update: NodeUpdate) -> Result<wg::Node> {
        let obj = self.node_obj(node_ref).await?;
        let precondition = match &update.etag {
            Some(etag) if !etag.is_empty() => Precondition::Version(
                etag.parse()
                    .map_err(|_| Error::invalid(format!("bad etag {etag:?}")))?,
            ),
            _ => Precondition::Any,
        };
        let mut props: NodeProps =
            serde_json::from_value(obj.properties.clone().unwrap_or_default())?;
        if let Some(spec) = update.spec {
            props.spec = serde_json::to_value(&spec)?;
        }
        if let Some(labels) = update.labels {
            props.labels = labels;
        }
        if let Some(phase) = update.phase {
            props.phase = phase.into();
        }
        let updated = self
            .store
            .update(
                &obj.id,
                Some(serde_json::to_value(&props)?),
                precondition,
                None,
            )
            .await
            .map_err(|e| match e {
                olai_store::Error::Conflict => {
                    Error::Conflict(format!("{node_ref} changed since etag {:?}", update.etag))
                }
                e => e.into(),
            })?;
        self.to_node(&updated)
    }

    async fn move_node(&self, node_ref: &str, from_parent: &str, to_parent: &str) -> Result<()> {
        let id = parse_ref(node_ref)?;
        let from = parse_ref(from_parent)?;
        let to = parse_ref(to_parent)?;
        let tx = self.store.begin().await?;
        tx.remove(id, from, crate::schema::CHILD_OF).await?;
        tx.add(id, to, crate::schema::CHILD_OF, None).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_nodes(&self, refs: &[String]) -> Result<()> {
        let ids = refs
            .iter()
            .map(|r| parse_ref(r))
            .collect::<Result<Vec<_>>>()?;
        let tx = self.store.begin().await?;
        for id in &ids {
            // Deleting an object deletes its associations with it.
            tx.delete(id).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn add_edge(&self, edge: &wg::Edge) -> Result<()> {
        let from = parse_ref(&edge.from)?;
        let to = parse_ref(&edge.to)?;
        let existing = edges_from(&self.store, from, &edge.kind).await?;
        if existing.iter().any(|a| a.to_id == to) {
            return Err(Error::AlreadyExists(format!(
                "{} {} {}",
                edge.from, edge.kind, edge.to
            )));
        }
        self.store.add(from, to, &edge.kind, None).await?;
        Ok(())
    }

    async fn remove_edge(&self, edge: &wg::Edge) -> Result<()> {
        let from = parse_ref(&edge.from)?;
        let to = parse_ref(&edge.to)?;
        let existing = edges_from(&self.store, from, &edge.kind).await?;
        if !existing.iter().any(|a| a.to_id == to) {
            return Err(Error::NotFound(format!(
                "{} {} {}",
                edge.from, edge.kind, edge.to
            )));
        }
        self.store.remove(from, to, &edge.kind).await?;
        Ok(())
    }
}

impl<S> OlaiBackend<S>
where
    S: ObjectStore<FactoryLabel> + AssociationStore<FactoryLabel> + Transactional<FactoryLabel>,
{
    /// The children of `node_ref` along `child_of`.
    pub async fn children(&self, node_ref: &str) -> Result<Vec<String>> {
        let id = parse_ref(node_ref)?;
        Ok(edges_into(&self.store, id, crate::schema::CHILD_OF)
            .await?
            .into_iter()
            .map(|a| to_ref(a.from_id))
            .collect())
    }
}

fn graph_segment(root: &str) -> Result<String> {
    Ok(parse_ref(root)?.to_string())
}

#[async_trait]
impl<S> RecordStore for OlaiBackend<S>
where
    S: ObjectStore<FactoryLabel> + AssociationStore<FactoryLabel> + Transactional<FactoryLabel>,
{
    async fn append_revision(&self, root: &str, revision: &StoredRevision) -> Result<()> {
        let name = ResourceName::new([graph_segment(root)?, format!("{:012}", revision.sequence)]);
        let props = serde_json::to_value(revision)?;
        self.store
            .create(FactoryLabel::Revision, &name, Some(props), None, None)
            .await?;
        Ok(())
    }

    async fn revisions(&self, root: &str) -> Result<Vec<StoredRevision>> {
        let mut revs: Vec<StoredRevision> = self
            .records(FactoryLabel::Revision, &[graph_segment(root)?])
            .await?;
        revs.sort_by_key(|r| r.sequence);
        Ok(revs)
    }

    async fn set_approved(&self, root: &str, root_hash: &str) -> Result<()> {
        let name = ResourceName::new([graph_segment(root)?]);
        self.put_record(
            FactoryLabel::Approval,
            name,
            &serde_json::json!({ "rootHash": root_hash }),
        )
        .await
    }

    async fn approved(&self, root: &str) -> Result<Option<String>> {
        let name = ResourceName::new([graph_segment(root)?]);
        match self.store.get_by_name(FactoryLabel::Approval, &name).await {
            Ok(obj) => Ok(obj
                .properties
                .and_then(|p| p.get("rootHash").and_then(Value::as_str).map(str::to_owned))),
            Err(olai_store::Error::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn add_annotation(&self, annotation: &wg::Annotation) -> Result<()> {
        let node = parse_ref(&annotation.node)?;
        let id = parse_ref(&annotation.r#ref)?;
        let name = ResourceName::new([node.to_string(), id.to_string()]);
        let props = serde_json::to_value(annotation)?;
        self.store
            .create(FactoryLabel::Annotation, &name, Some(props), Some(id), None)
            .await?;
        Ok(())
    }

    async fn annotations(&self, node_ref: &str) -> Result<Vec<wg::Annotation>> {
        self.records(
            FactoryLabel::Annotation,
            &[parse_ref(node_ref)?.to_string()],
        )
        .await
    }

    async fn put_run(&self, root: &str, run: &wg::Run) -> Result<()> {
        let id = parse_ref(&run.r#ref)?;
        let name = ResourceName::new([graph_segment(root)?, id.to_string()]);
        self.put_record(FactoryLabel::Run, name, run).await
    }

    async fn runs(&self, root: &str) -> Result<Vec<wg::Run>> {
        self.records(FactoryLabel::Run, &[graph_segment(root)?])
            .await
    }

    async fn append_event(&self, root: &str, event: &Event) -> Result<()> {
        let name = ResourceName::new([graph_segment(root)?, Uuid::now_v7().to_string()]);
        let props = serde_json::to_value(event)?;
        self.store
            .create(FactoryLabel::Event, &name, Some(props), None, None)
            .await?;
        Ok(())
    }

    async fn events(&self, root: &str) -> Result<Vec<Event>> {
        self.records(FactoryLabel::Event, &[graph_segment(root)?])
            .await
    }

    async fn put_attention(&self, root: &str, item: &wg::Attention) -> Result<()> {
        let id = parse_ref(&item.r#ref)?;
        let name = ResourceName::new([graph_segment(root)?, id.to_string()]);
        self.put_record(FactoryLabel::Attention, name, item).await
    }

    async fn attention(&self, root: &str) -> Result<Vec<wg::Attention>> {
        let mut items: Vec<wg::Attention> = self
            .records(FactoryLabel::Attention, &[graph_segment(root)?])
            .await?;
        // Refs are v7 UUIDs: oldest first.
        items.sort_by(|a, b| a.r#ref.cmp(&b.r#ref));
        Ok(items)
    }
}

/// A new ref for a record (an annotation, a run) in this backend's format.
pub fn new_ref() -> String {
    to_ref(Uuid::now_v7())
}

/// An olai-store backend in memory, for tests and demos.
pub fn in_memory(schema: &Schema) -> OlaiBackend<olai_store::InMemoryStore<FactoryLabel>> {
    OlaiBackend::new(olai_store::InMemoryStore::new(), schema)
}

/// An olai-store backend on SQLite. `url` is a sqlx URL
/// (`sqlite://factory.db?mode=rwc`, or `sqlite::memory:`); migrations run on
/// connect.
#[cfg(feature = "sqlite")]
pub async fn sqlite(
    url: &str,
    schema: &Schema,
) -> Result<OlaiBackend<olai_store::SqlStore<FactoryLabel>>> {
    let store = if url == "sqlite::memory:" {
        olai_store::SqlStore::in_memory().await?
    } else {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(url)
            .await
            .map_err(Error::internal)?;
        olai_store::SqlStore::connect_and_migrate(pool).await?
    };
    Ok(OlaiBackend::new(store, schema))
}

/// An olai-store backend on Postgres, over `pool`. Never migrates: apply
/// [`postgres_migrator`] once, as a deploy step, and check it at startup
/// with [`postgres_check`].
#[cfg(feature = "postgres")]
pub fn postgres(
    pool: sqlx::PgPool,
    schema: &Schema,
) -> OlaiBackend<olai_store::PgStore<FactoryLabel>> {
    OlaiBackend::new(olai_store::PgStore::connect(pool), schema)
}

/// The Postgres backend's schema migrations (olai-store's), for whoever
/// applies them.
#[cfg(feature = "postgres")]
pub fn postgres_migrator() -> sqlx::migrate::Migrator {
    olai_store::backend::postgres::migrator()
}

/// Fails unless the database behind `pool` has exactly the migrations
/// [`postgres_migrator`] would apply, with the same checksums: none pending,
/// none unknown, none dirty. A read of the ledger, without the migration lock.
#[cfg(feature = "postgres")]
pub async fn postgres_check(pool: &sqlx::PgPool) -> Result<()> {
    use std::collections::HashMap;

    let rows: Vec<(i64, Vec<u8>, bool)> = match sqlx::query_as(
        "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("42P01") => {
            return Err(Error::internal(
                "the work graph's schema isn't applied; run `luminescent-migrate workgraph`",
            ));
        }
        Err(e) => return Err(Error::internal(e)),
    };
    let applied: HashMap<i64, (Vec<u8>, bool)> =
        rows.into_iter().map(|(v, c, s)| (v, (c, s))).collect();
    let migrator = postgres_migrator();
    let expected: Vec<_> = migrator
        .iter()
        .filter(|m| m.migration_type.is_up_migration())
        .collect();
    for m in &expected {
        match applied.get(&m.version) {
            None => {
                return Err(Error::internal(format!(
                    "migration {} ({}) is pending; run `luminescent-migrate workgraph`",
                    m.version, m.description
                )));
            }
            Some((_, false)) => {
                return Err(Error::internal(format!(
                    "migration {} failed part-way; fix the database by hand",
                    m.version
                )));
            }
            Some((checksum, true)) if checksum.as_slice() != m.checksum.as_ref() => {
                return Err(Error::internal(format!(
                    "migration {} was changed after it was applied",
                    m.version
                )));
            }
            Some(_) => {}
        }
    }
    if let Some(v) = applied
        .keys()
        .find(|v| !expected.iter().any(|m| m.version == **v))
    {
        return Err(Error::internal(format!(
            "the database has migration {v}, which this build doesn't know: deploy a newer one"
        )));
    }
    Ok(())
}
