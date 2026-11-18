//! The extension seam: what a composition root plugs into the framework.
//!
//! An [`Extension`] contributes node and edge kinds, the gate conditions and
//! task extensions it understands, and the descriptors and proto-JSON types
//! of its payloads. A [`FactoryBuilder`] merges the built-in extension (the
//! reference kind set, [`crate::builtin`]) with whatever the binary adds,
//! and builds a [`Factory`] whose writes are checked against the merged set.
//!
//! The rule of thumb: when the line of work differs, add a node kind with
//! its own executor. When it's the same line with extra intent, ride on a
//! `Task` as an extension payload, interpreted by a [`TaskHook`].
//!
//! Executors and MCP tools are registered here too, but nothing drives them
//! yet: the reconciler and `workgraph-mcp` fill them in.

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;

use buffa::{Message, MessageName};
use buffa_descriptor::DescriptorPool;
use buffa_descriptor::generated::descriptor::FileDescriptorSet;
use buffa_types::google::protobuf::Any;
use factory_proto::AddTypes;
use factory_proto::workgraph::v1 as wg;

use crate::backend::Backend;
use crate::schema::Schema;
use crate::{Error, Factory, Result};

/// Checks a node payload's bytes, with the merged extensions at hand (so a
/// kind can check the `Any`s it carries against what's registered).
pub type Validate = fn(&[u8], &Extensions) -> Result<()>;

/// A node kind and how its payload is checked on write.
#[derive(Clone)]
pub struct KindSpec {
    pub kind: wg::NodeKind,
    pub validate: Validate,
}

fn decodes<M: Message + MessageName + Default>(bytes: &[u8], _: &Extensions) -> Result<()> {
    M::decode_from_slice(bytes)
        .map(drop)
        .map_err(|_| Error::invalid(format!("payload doesn't decode as {}", M::FULL_NAME)))
}

impl KindSpec {
    /// A kind whose payload is `M`, checked by decoding it.
    pub fn of<M: Message + MessageName + Default>(kind: wg::NodeKind) -> Self {
        Self {
            kind: wg::NodeKind {
                kind: M::TYPE_URL.into(),
                ..kind
            },
            validate: decodes::<M>,
        }
    }

    /// A kind with its own payload check.
    pub fn with(kind: wg::NodeKind, validate: Validate) -> Self {
        Self { kind, validate }
    }
}

/// A condition a `Gate` node may carry, keyed by its type URL.
///
/// Today an evaluator declares and checks its condition. The reconciler adds
/// `evaluate`: whether the gate is open, given the graph and its runs.
pub trait GateEvaluator: Send + Sync {
    /// The condition message's type URL.
    fn condition_type(&self) -> &str;
    fn display_name(&self) -> &str;
    fn description(&self) -> &str {
        ""
    }
    /// Check a condition before it's stored.
    fn validate(&self, condition: &Any) -> Result<()>;
}

/// Interprets one kind of `Task` extension payload, keyed by its type URL.
///
/// Today a hook declares and checks its payload. The reconciler adds the
/// points where it reacts (grouping a stack's PRs, waiting for a release).
pub trait TaskHook: Send + Sync {
    /// The extension message's type URL.
    fn extension_type(&self) -> &str;
    fn display_name(&self) -> &str;
    fn description(&self) -> &str {
        ""
    }
    /// Check an extension payload before it's stored.
    fn validate(&self, extension: &Any) -> Result<()>;
}

/// Runs the nodes of one kind. A placeholder: the reconciler gives it the
/// driving methods.
pub trait NodeExecutor: Send + Sync {
    /// The node kind's type URL.
    fn kind(&self) -> &str;
}

/// A tool an extension adds to `workgraph-mcp`, and which roles may call
/// it. A placeholder: `workgraph-mcp` gives it a schema and a handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// The roles allowed to call it; empty for none.
    pub roles: Vec<String>,
}

/// A payload message declared as a gate condition or a task extension,
/// checked by decoding it, with no behavior yet.
pub struct Declared<M> {
    display_name: &'static str,
    description: &'static str,
    _message: PhantomData<fn() -> M>,
}

impl<M> Declared<M> {
    pub fn new(display_name: &'static str, description: &'static str) -> Self {
        Self {
            display_name,
            description,
            _message: PhantomData,
        }
    }
}

fn decode_any<M: Message + MessageName + Default>(any: &Any) -> Result<()> {
    if any.type_url != M::TYPE_URL {
        return Err(Error::invalid(format!(
            "expected {}, got {:?}",
            M::TYPE_URL,
            any.type_url
        )));
    }
    M::decode_from_slice(&any.value)
        .map(drop)
        .map_err(|_| Error::invalid(format!("doesn't decode as {}", M::FULL_NAME)))
}

impl<M: Message + MessageName + Default> GateEvaluator for Declared<M> {
    fn condition_type(&self) -> &str {
        M::TYPE_URL
    }
    fn display_name(&self) -> &str {
        self.display_name
    }
    fn description(&self) -> &str {
        self.description
    }
    fn validate(&self, condition: &Any) -> Result<()> {
        decode_any::<M>(condition)
    }
}

impl<M: Message + MessageName + Default> TaskHook for Declared<M> {
    fn extension_type(&self) -> &str {
        M::TYPE_URL
    }
    fn display_name(&self) -> &str {
        self.display_name
    }
    fn description(&self) -> &str {
        self.description
    }
    fn validate(&self, extension: &Any) -> Result<()> {
        decode_any::<M>(extension)
    }
}

/// What an extension contributes. Every method has an empty default, so an
/// extension names only what it adds.
pub trait Extension: Send + Sync {
    /// A short, stable name (`builtin`), shown with what it registers.
    fn name(&self) -> &'static str;
    fn kinds(&self) -> Vec<KindSpec> {
        Vec::new()
    }
    fn edge_kinds(&self) -> Vec<wg::EdgeKind> {
        Vec::new()
    }
    fn gates(&self) -> Vec<Box<dyn GateEvaluator>> {
        Vec::new()
    }
    fn task_hooks(&self) -> Vec<Box<dyn TaskHook>> {
        Vec::new()
    }
    fn executors(&self) -> Vec<Box<dyn NodeExecutor>> {
        Vec::new()
    }
    fn mcp_tools(&self) -> Vec<ToolSpec> {
        Vec::new()
    }
    /// The generated `register_types` of each package the extension's
    /// payloads come from, so `Any`s holding them serialize as proto-JSON
    /// (and hash).
    fn types(&self) -> Vec<AddTypes> {
        Vec::new()
    }
    /// An encoded `FileDescriptorSet` with the extension's payload types, so
    /// clients can show payloads they have no code for. Files another
    /// extension already brings (its imports) may be left out.
    fn descriptors(&self) -> &'static [u8] {
        &[]
    }
}

/// The merged result of every extension: the schema writes are checked
/// against, and the registered conditions, hooks, executors and tools.
pub struct Extensions {
    names: Vec<&'static str>,
    schema: Schema,
    validators: HashMap<String, Validate>,
    gates: HashMap<String, Box<dyn GateEvaluator>>,
    hooks: HashMap<String, Box<dyn TaskHook>>,
    executors: HashMap<String, Box<dyn NodeExecutor>>,
    tools: Vec<ToolSpec>,
    descriptors: Vec<u8>,
}

impl std::fmt::Debug for Extensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extensions")
            .field("names", &self.names)
            .field("schema", &self.schema)
            .finish_non_exhaustive()
    }
}

impl Extensions {
    /// The built-in extension alone.
    pub fn builtin() -> Self {
        FactoryBuilder::new()
            .extensions()
            .expect("the built-in extension merges")
    }

    /// The extensions' names, in registration order.
    pub fn names(&self) -> &[&'static str] {
        &self.names
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn gate(&self, condition_type: &str) -> Option<&dyn GateEvaluator> {
        self.gates.get(condition_type).map(AsRef::as_ref)
    }

    pub fn task_hook(&self, extension_type: &str) -> Option<&dyn TaskHook> {
        self.hooks.get(extension_type).map(AsRef::as_ref)
    }

    pub fn executor(&self, kind: &str) -> Option<&dyn NodeExecutor> {
        self.executors.get(kind).map(AsRef::as_ref)
    }

    pub fn mcp_tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// Every extension's descriptors as one encoded `FileDescriptorSet`,
    /// each file once.
    pub fn descriptors(&self) -> &[u8] {
        &self.descriptors
    }

    /// Check a node payload of `kind`. A kind registered without a check
    /// can't exist, so an unknown kind is an error.
    pub fn validate_payload(&self, kind: &str, bytes: &[u8]) -> Result<()> {
        let validate = self
            .validators
            .get(kind)
            .ok_or_else(|| Error::invalid(format!("unknown node kind {kind:?}")))?;
        validate(bytes, self)
    }

    /// Check a gate condition against the registered evaluators.
    pub fn validate_condition(&self, condition: &Any) -> Result<()> {
        let gate = self.gate(&condition.type_url).ok_or_else(|| {
            Error::invalid(format!(
                "gate condition {:?} isn't registered; is its extension loaded?",
                condition.type_url
            ))
        })?;
        gate.validate(condition)
    }

    /// Check a task extension against the registered hooks.
    pub fn validate_task_extension(&self, extension: &Any) -> Result<()> {
        let hook = self.task_hook(&extension.type_url).ok_or_else(|| {
            Error::invalid(format!(
                "task extension {:?} isn't registered; is its extension loaded?",
                extension.type_url
            ))
        })?;
        hook.validate(extension)
    }
}

/// Merges extensions into a [`Factory`]. Starts with the built-in one.
pub struct FactoryBuilder {
    extensions: Vec<Arc<dyn Extension>>,
}

impl Default for FactoryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FactoryBuilder {
    /// A builder with the built-in extension (the reference kind set).
    pub fn new() -> Self {
        Self {
            extensions: vec![Arc::new(crate::builtin::Builtin)],
        }
    }

    /// Add an extension after the ones already added.
    #[must_use]
    pub fn extension(mut self, extension: impl Extension + 'static) -> Self {
        self.extensions.push(Arc::new(extension));
        self
    }

    /// The merged schema alone, which a backend is opened with. Fails when
    /// two extensions register the same kind, edge kind, condition or
    /// extension type.
    pub fn schema(&self) -> Result<Schema> {
        Ok(self.merge()?.schema)
    }

    /// Merge every extension, install their proto-JSON types, and merge
    /// their descriptors.
    pub fn extensions(&self) -> Result<Extensions> {
        let mut merged = self.merge()?;
        let types: Vec<AddTypes> = self.extensions.iter().flat_map(|e| e.types()).collect();
        factory_proto::install_types(&types);
        merged.descriptors = self.merge_descriptors()?;
        Ok(merged)
    }

    /// The factory over `backend`, which must have been opened with
    /// [`schema`](Self::schema).
    pub fn build(&self, backend: Arc<dyn Backend>) -> Result<Factory> {
        Ok(Factory::new(backend, self.extensions()?))
    }

    fn merge(&self) -> Result<Extensions> {
        let mut out = Extensions {
            names: Vec::new(),
            schema: Schema::new(wg::Schema::default()),
            validators: HashMap::new(),
            gates: HashMap::new(),
            hooks: HashMap::new(),
            executors: HashMap::new(),
            tools: Vec::new(),
            descriptors: Vec::new(),
        };
        let mut proto = wg::Schema::default();
        let mut owner: HashMap<String, &'static str> = HashMap::new();
        let mut claim = |what: &str, key: &str, ext: &'static str| -> Result<()> {
            let id = format!("{what} {key}");
            match owner.insert(id, ext) {
                Some(prev) => Err(Error::invalid(format!(
                    "extension {ext:?} registers {what} {key:?}, which {prev:?} already does"
                ))),
                None => Ok(()),
            }
        };
        let mut names = HashSet::new();
        for e in &self.extensions {
            let name = e.name();
            if !names.insert(name) {
                return Err(Error::invalid(format!("extension {name:?} is added twice")));
            }
            out.names.push(name);
            for spec in e.kinds() {
                claim("node kind", &spec.kind.kind, name)?;
                out.validators.insert(spec.kind.kind.clone(), spec.validate);
                proto.node_kinds.push(wg::NodeKind {
                    extension: name.into(),
                    ..spec.kind
                });
            }
            for kind in e.edge_kinds() {
                claim("edge kind", &kind.name, name)?;
                proto.edge_kinds.push(kind);
            }
            for gate in e.gates() {
                let url = gate.condition_type().to_owned();
                claim("gate condition", &url, name)?;
                proto.gate_conditions.push(payload_kind(
                    &url,
                    gate.display_name(),
                    gate.description(),
                    name,
                ));
                out.gates.insert(url, gate);
            }
            for hook in e.task_hooks() {
                let url = hook.extension_type().to_owned();
                claim("task extension", &url, name)?;
                proto.task_extensions.push(payload_kind(
                    &url,
                    hook.display_name(),
                    hook.description(),
                    name,
                ));
                out.hooks.insert(url, hook);
            }
            for executor in e.executors() {
                let kind = executor.kind().to_owned();
                claim("executor", &kind, name)?;
                out.executors.insert(kind, executor);
            }
            for tool in e.mcp_tools() {
                claim("MCP tool", &tool.name, name)?;
                out.tools.push(tool);
            }
        }
        out.schema = Schema::new(proto);
        Ok(out)
    }

    fn merge_descriptors(&self) -> Result<Vec<u8>> {
        let mut pool = DescriptorPool::default();
        for e in &self.extensions {
            let bytes = e.descriptors();
            if bytes.is_empty() {
                continue;
            }
            let set = FileDescriptorSet::decode_from_slice(bytes).map_err(|err| {
                Error::internal(format!("{}'s descriptors don't decode: {err}", e.name()))
            })?;
            pool.add_file_descriptor_set(set).map_err(|err| {
                Error::internal(format!("{}'s descriptors don't resolve: {err}", e.name()))
            })?;
        }
        Ok(FileDescriptorSet {
            file: pool.files().to_vec(),
            ..Default::default()
        }
        .encode_to_vec())
    }
}

fn payload_kind(type_url: &str, name: &str, description: &str, ext: &str) -> wg::PayloadKind {
    wg::PayloadKind {
        type_url: type_url.into(),
        display_name: name.into(),
        description: description.into(),
        extension: ext.into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{GATE, TASK};

    struct Again;
    impl Extension for Again {
        fn name(&self) -> &'static str {
            "again"
        }
        fn kinds(&self) -> Vec<KindSpec> {
            vec![KindSpec::of::<wg::Task>(wg::NodeKind::default())]
        }
    }

    struct Notes;
    impl Extension for Notes {
        fn name(&self) -> &'static str {
            "notes"
        }
        fn kinds(&self) -> Vec<KindSpec> {
            vec![KindSpec::of::<wg::Learning>(wg::NodeKind {
                display_name: "Note".into(),
                ..Default::default()
            })]
        }
        fn task_hooks(&self) -> Vec<Box<dyn TaskHook>> {
            vec![Box::new(Declared::<wg::ReconcileNote>::new("Note", ""))]
        }
        fn mcp_tools(&self) -> Vec<ToolSpec> {
            vec![ToolSpec {
                name: "note".into(),
                description: String::new(),
                roles: vec!["planner".into()],
            }]
        }
    }

    #[test]
    fn builtin_is_the_reference_set() {
        let ext = Extensions::builtin();
        assert_eq!(ext.names(), ["builtin"]);
        let kinds: Vec<&str> = ext
            .schema()
            .proto()
            .node_kinds
            .iter()
            .map(|k| k.kind.as_str())
            .collect();
        assert_eq!(kinds.len(), 4);
        assert!(kinds.contains(&TASK) && kinds.contains(&GATE));
        assert!(ext.gate(wg::PrMerged::TYPE_URL).is_some());
        let pool = DescriptorPool::decode(ext.descriptors()).unwrap();
        assert!(pool.message_by_name("factory.workgraph.v1.Task").is_some());
    }

    #[test]
    fn a_kind_registered_twice_is_refused() {
        let err = FactoryBuilder::new().extension(Again).schema().unwrap_err();
        assert!(err.to_string().contains("already does"), "{err}");
    }

    #[test]
    fn an_extension_is_added_once() {
        let err = FactoryBuilder::new()
            .extension(Notes)
            .extension(Notes)
            .schema()
            .unwrap_err();
        assert!(err.to_string().contains("added twice"), "{err}");
    }

    #[test]
    fn extensions_merge() {
        let ext = FactoryBuilder::new().extension(Notes).extensions().unwrap();
        assert_eq!(ext.names(), ["builtin", "notes"]);
        let note = ext.schema().node_kind(wg::Learning::TYPE_URL).unwrap();
        assert_eq!(note.extension, "notes");
        assert_eq!(
            ext.schema()
                .task_extension(wg::ReconcileNote::TYPE_URL)
                .unwrap()
                .extension,
            "notes"
        );
        assert_eq!(ext.mcp_tools()[0].roles, ["planner"]);
        ext.validate_payload(wg::Learning::TYPE_URL, &[]).unwrap();
        assert!(
            ext.validate_payload("type.googleapis.com/x.Y", &[])
                .is_err()
        );
        let bad = Any {
            type_url: wg::Learning::TYPE_URL.into(),
            ..Default::default()
        };
        assert!(
            ext.validate_task_extension(&bad).is_err(),
            "not a hook's type"
        );
        assert!(ext.validate_condition(&bad).is_err(), "not a condition");
    }

    #[test]
    fn gates_take_registered_conditions_only() {
        let ext = Extensions::builtin();
        let gate = wg::Gate {
            condition: crate::payload::pack(&wg::HumanGate::default()).into(),
            ..Default::default()
        };
        ext.validate_payload(GATE, &gate.encode_to_vec()).unwrap();
        ext.validate_payload(GATE, &[]).unwrap();
        assert!(ext.validate_payload(GATE, &[0xff, 0xff]).is_err());
        let unknown = wg::Gate {
            condition: crate::payload::pack(&wg::Learning::default()).into(),
            ..Default::default()
        };
        let err = ext
            .validate_payload(GATE, &unknown.encode_to_vec())
            .unwrap_err();
        assert!(err.to_string().contains("isn't registered"), "{err}");
    }

    #[test]
    fn tasks_take_registered_extensions_only() {
        let task = |e: &[Any]| {
            wg::Task {
                extensions: e.to_vec(),
                ..Default::default()
            }
            .encode_to_vec()
        };
        let note = crate::payload::pack(&wg::ReconcileNote::default());
        let builtin = Extensions::builtin();
        builtin.validate_payload(TASK, &task(&[])).unwrap();
        assert!(
            builtin
                .validate_payload(TASK, &task(std::slice::from_ref(&note)))
                .is_err()
        );
        let ext = FactoryBuilder::new().extension(Notes).extensions().unwrap();
        ext.validate_payload(TASK, &task(&[note])).unwrap();
    }
}
