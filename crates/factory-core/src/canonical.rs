//! The plan's canonical forms: the Merkle tree a revision hashes, and a
//! deterministic markdown rendering for text diffs.
//!
//! Both are computed from the normalized `factory.workgraph.v1` model, never
//! from a backend's representation, so a plan keeps its hash whichever
//! backend holds it.

use std::collections::{BTreeMap, HashMap};

use factory_proto::workgraph::v1 as wg;
use plan_tree::NodeInput;
use serde_json::Value;

use crate::backend::TextSection;
use crate::schema::{self, Schema};
use crate::{Error, Result};

/// The graph indexed for traversal.
pub struct Indexed<'g> {
    pub graph: &'g wg::Graph,
    pub nodes: HashMap<&'g str, &'g wg::Node>,
    /// Children by parent, sorted by ref.
    pub children: HashMap<&'g str, Vec<&'g str>>,
    pub parent: HashMap<&'g str, &'g str>,
    /// Outgoing edges by source, then kind, targets sorted.
    pub out: HashMap<&'g str, HashMap<&'g str, Vec<&'g str>>>,
}

impl<'g> Indexed<'g> {
    pub fn new(graph: &'g wg::Graph) -> Self {
        let nodes = graph.nodes.iter().map(|n| (n.r#ref.as_str(), n)).collect();
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut parent = HashMap::new();
        let mut out: HashMap<&str, HashMap<&str, Vec<&str>>> = HashMap::new();
        for e in &graph.edges {
            if e.kind == schema::CHILD_OF {
                children
                    .entry(e.to.as_str())
                    .or_default()
                    .push(e.from.as_str());
                parent.insert(e.from.as_str(), e.to.as_str());
            }
            out.entry(e.from.as_str())
                .or_default()
                .entry(e.kind.as_str())
                .or_default()
                .push(e.to.as_str());
        }
        for v in children
            .values_mut()
            .chain(out.values_mut().flat_map(|m| m.values_mut()))
        {
            v.sort_unstable();
        }
        Self {
            graph,
            nodes,
            children,
            parent,
            out,
        }
    }

    pub fn children_of(&self, node: &str) -> &[&'g str] {
        self.children.get(node).map_or(&[], Vec::as_slice)
    }

    pub fn targets(&self, node: &str, kind: &str) -> &[&'g str] {
        self.out
            .get(node)
            .and_then(|m| m.get(kind))
            .map_or(&[], Vec::as_slice)
    }

    /// `node`'s ancestors, nearest first.
    pub fn ancestors(&self, node: &str) -> Vec<&'g str> {
        let mut out = Vec::new();
        let mut cur = self.parent.get(node).copied();
        while let Some(p) = cur {
            if out.contains(&p) {
                break;
            }
            out.push(p);
            cur = self.parent.get(p).copied();
        }
        out
    }

    /// Everything below `node`, depth first.
    pub fn descendants(&self, node: &str) -> Vec<&'g str> {
        let mut out = Vec::new();
        let mut stack: Vec<&str> = self.children_of(node).iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.children_of(n).iter().rev());
        }
        out
    }
}

/// One line: whitespace runs collapsed, trimmed.
pub fn norm_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text: `\n` line endings, no trailing whitespace on a line, no leading or
/// trailing blank lines.
pub fn norm_text(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim_end).collect();
    let start = lines
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(start, |i| i + 1);
    lines[start..end].join("\n")
}

/// A JSON value with object keys sorted, strings normalized and arrays of
/// strings sorted (repeated string fields are sets).
fn canonical_value(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let sorted: BTreeMap<&String, Value> =
                m.iter().map(|(k, v)| (k, canonical_value(v))).collect();
            Value::Object(sorted.into_iter().map(|(k, v)| (k.clone(), v)).collect())
        }
        Value::Array(a) => {
            let mut items: Vec<Value> = a.iter().map(canonical_value).collect();
            if items.iter().all(Value::is_string) {
                items.sort_by(|x, y| x.as_str().cmp(&y.as_str()));
            }
            Value::Array(items)
        }
        Value::String(s) => Value::String(norm_text(s)),
        other => other.clone(),
    }
}

/// A payload as proto-JSON. An `Any` inside it of a type that isn't
/// registered (its extension isn't loaded) falls back to
/// `{"@type", "value": <base64>}`. Writes refuse such types, so this only
/// happens when a server without the extension reads a plan written with
/// it. The hash then differs from the one recorded with the extension, so
/// the node reads as drifted, and nothing dispatches against it unnoticed.
fn payload_json(any: &buffa_types::google::protobuf::Any) -> Result<Value> {
    Ok(serde_json::to_value(any)?)
}

/// The payload as canonical JSON (sorted keys, normalized text), or `""`.
pub fn canonical_payload(spec: &wg::NodeSpec) -> Result<String> {
    let Some(any) = spec.payload.as_option() else {
        return Ok(String::new());
    };
    let v = payload_json(any)?;
    // serde_json's map keeps insertion order with `preserve_order`, so the
    // order is fixed here rather than relied on.
    Ok(write_sorted(&canonical_value(&v)))
}

fn write_sorted(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), write_sorted(&m[k])))
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(write_sorted).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

/// The payload's fields as canonical JSON, without `@type`.
fn payload_fields(spec: &wg::NodeSpec) -> Result<BTreeMap<String, Value>> {
    let Some(any) = spec.payload.as_option() else {
        return Ok(BTreeMap::new());
    };
    let v = canonical_value(&payload_json(any)?);
    Ok(match v {
        Value::Object(m) => m.into_iter().filter(|(k, _)| k != "@type").collect(),
        _ => BTreeMap::new(),
    })
}

fn spec_of(node: &wg::Node) -> &wg::NodeSpec {
    &node.spec
}

/// The Merkle tree input for a graph: one node per work-graph node, keyed by
/// ref, children along `child_of` (sorted by key when built), and each
/// node's own parts:
///
/// `[kind, title, body, payload, (edge kind, n, targets…) per hashed edge kind]`
///
/// Phase, labels, metadata and unhashed edges are left out.
pub fn tree(graph: &wg::Graph, schema: &Schema) -> Result<NodeInput> {
    let idx = Indexed::new(graph);
    if !idx.nodes.contains_key(graph.root.as_str()) {
        return Err(Error::NotFound(graph.root.clone()));
    }
    let hashed: Vec<&str> = {
        let mut v: Vec<&str> = schema.hashed_edge_kinds().collect();
        v.sort_unstable();
        v
    };
    fn build(
        idx: &Indexed<'_>,
        hashed: &[&str],
        node_ref: &str,
        depth: usize,
    ) -> Result<NodeInput> {
        if depth > 64 {
            return Err(Error::internal("child_of nesting deeper than 64"));
        }
        let node = idx.nodes[node_ref];
        let spec = spec_of(node);
        let mut parts = vec![
            node.kind.clone(),
            norm_line(&spec.title),
            norm_text(&spec.body),
            canonical_payload(spec)?,
        ];
        for kind in hashed {
            let targets = idx.targets(node_ref, kind);
            parts.push((*kind).to_owned());
            parts.push(targets.len().to_string());
            parts.extend(targets.iter().map(|t| (*t).to_owned()));
        }
        let mut input = NodeInput::new(
            node_ref,
            schema::short_kind(&node.kind),
            norm_line(&spec.title),
        )
        .parts(parts)
        .attr("title", norm_line(&spec.title))
        .attr("kind", schema::short_kind(&node.kind));
        for child in idx.children_of(node_ref) {
            input.children.push(build(idx, hashed, child, depth + 1)?);
        }
        Ok(input)
    }
    build(&idx, &hashed, &graph.root, 0)
}

/// A graph rendered as markdown: one section per node, in tree order
/// (children sorted by ref), with the payload's fields in key order.
pub struct PlanText {
    pub text: String,
    pub sections: Vec<TextSection>,
}

pub fn render(graph: &wg::Graph, schema: &Schema) -> Result<PlanText> {
    let idx = Indexed::new(graph);
    if !idx.nodes.contains_key(graph.root.as_str()) {
        return Err(Error::NotFound(graph.root.clone()));
    }
    let mut lines: Vec<String> = Vec::new();
    let mut sections = Vec::new();
    let title_of = |r: &str| {
        idx.nodes
            .get(r)
            .map_or_else(|| r.to_owned(), |n| norm_line(&n.spec.title))
    };
    let mut stack = vec![(graph.root.as_str(), 1usize)];
    while let Some((node_ref, depth)) = stack.pop() {
        let node = idx.nodes[node_ref];
        let spec = spec_of(node);
        if !lines.is_empty() {
            lines.push(String::new());
        }
        let start = lines.len() as u32 + 1;
        let kind_name = schema.node_kind(&node.kind).map_or_else(
            || schema::short_kind(&node.kind).to_owned(),
            |k| k.display_name.clone(),
        );
        lines.push(format!(
            "{} {kind_name}: {}",
            "#".repeat(depth.min(6)),
            norm_line(&spec.title)
        ));
        for kind in schema.edge_kinds().filter(|k| k.name != schema::CHILD_OF) {
            let targets = idx.targets(node_ref, &kind.name);
            if !targets.is_empty() {
                let names: Vec<String> = targets.iter().map(|t| title_of(t)).collect();
                lines.push(format!(
                    "{}: {}",
                    kind.name.replace('_', " "),
                    names.join("; ")
                ));
            }
        }
        for (field, value) in payload_fields(spec)? {
            render_field(&mut lines, &field, &value);
        }
        let body = norm_text(&spec.body);
        if !body.is_empty() {
            lines.push(String::new());
            lines.extend(body.lines().map(str::to_owned));
        }
        sections.push(TextSection {
            key: node_ref.to_owned(),
            start_line: start,
            end_line: lines.len() as u32,
        });
        for child in idx.children_of(node_ref).iter().rev() {
            stack.push((child, depth + 1));
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    Ok(PlanText { text, sections })
}

fn render_field(lines: &mut Vec<String>, field: &str, value: &Value) {
    match value {
        Value::String(s) if s.contains('\n') => {
            lines.push(format!("{field}:"));
            lines.extend(s.lines().map(|l| format!("  {l}")));
        }
        Value::String(s) => lines.push(format!("{field}: {s}")),
        Value::Array(items) => {
            lines.push(format!("{field}:"));
            for item in items {
                match item {
                    Value::String(s) => lines.push(format!("- {s}")),
                    other => lines.push(format!("- {other}")),
                }
            }
        }
        other => lines.push(format!("{field}: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_normalization() {
        assert_eq!(norm_line("  a \t b\n c "), "a b c");
        assert_eq!(norm_text("\n\n a  \r\n b\n\n"), " a\n b");
    }

    fn task_spec(
        extensions: Vec<buffa_types::google::protobuf::Any>,
        paths: &[&str],
    ) -> wg::NodeSpec {
        crate::payload::spec(
            "T",
            "",
            &wg::Task {
                extensions,
                ..crate::payload::task("a", paths)
            },
        )
    }

    #[test]
    fn task_extensions_hash_in_order() {
        factory_proto::register_types();
        let a = crate::payload::pack(&wg::ReconcileNote {
            text: "a".into(),
            ..Default::default()
        });
        let b = crate::payload::pack(&wg::Learning {
            lesson: "b".into(),
            ..Default::default()
        });
        let ab = canonical_payload(&task_spec(vec![a.clone(), b.clone()], &["x", "y"])).unwrap();
        let ba = canonical_payload(&task_spec(vec![b, a.clone()], &["x", "y"])).unwrap();
        assert_ne!(ab, ba, "extensions are ordered");
        assert!(ab.contains(r#""text":"a""#), "{ab}");
        let a_xy = canonical_payload(&task_spec(vec![a.clone()], &["x", "y"])).unwrap();
        let a_yx = canonical_payload(&task_spec(vec![a], &["y", "x"])).unwrap();
        assert_eq!(a_xy, a_yx, "touched paths are a set");
    }

    #[test]
    fn an_unregistered_payload_type_hashes_by_its_bytes() {
        factory_proto::register_types();
        let unknown = buffa_types::google::protobuf::Any {
            type_url: "type.googleapis.com/nowhere.v1.Thing".into(),
            value: vec![8, 1].into(),
            ..Default::default()
        };
        let json = canonical_payload(&task_spec(vec![unknown], &[])).unwrap();
        assert!(
            json.contains(r#"{"@type":"type.googleapis.com/nowhere.v1.Thing","value":"CAE="}"#),
            "{json}"
        );
    }

    #[test]
    fn canonical_json_sorts_keys_and_string_sets() {
        let v: Value = serde_json::json!({"b": ["z", "a"], "a": {"d": 1, "c": "x  \n"}});
        assert_eq!(
            write_sorted(&canonical_value(&v)),
            r#"{"a":{"c":"x","d":1},"b":["a","z"]}"#
        );
    }
}
