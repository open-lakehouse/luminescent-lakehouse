//! The built-in tools: generic operations on the work graph, with no
//! workflow meaning of their own. When an agent proposes or submits, and
//! what happens next, is the composition's to decide (its agents' prompts,
//! its executors).
//!
//! Arguments are JSON. Payloads are proto-JSON: an `Any` carries its
//! `@type`, which the tools fill in from the kind when it's left out. A
//! result's `structuredContent` is JSON; the reads and the plan's writes put
//! markdown in its text ([`crate::render`]), the others the JSON again.

use std::collections::HashMap;

use async_trait::async_trait;
use buffa_types::google::protobuf::Any;
use factory_core::canonical::Indexed;
use factory_core::schema::{self, LEARNING, RECONCILE_NOTE, short_kind};
use factory_core::tool::{ToolCall, ToolHandler, ToolOutput, ToolSpec};
use factory_core::{Error, Result, condition, payload};
use factory_proto::workgraph::v1 as wg;
use serde_json::{Map, Value, json};

use crate::render::{self, holds, phase};

/// The names of the built-in tools. An extension can't reuse them.
pub const BUILTIN: [&str; 15] = [
    "plan_read",
    "item_read",
    "ready",
    "add_item",
    "update_item",
    "add_edge",
    "annotate",
    "propose",
    "report_change",
    "list_repos",
    "list_profiles",
    "submit",
    "open_plan_review",
    "approve_revision",
    "return_plan",
];

#[derive(Clone, Copy)]
enum Op {
    PlanRead,
    ItemRead,
    Ready,
    AddItem,
    UpdateItem,
    AddEdge,
    Annotate,
    Propose,
    ReportChange,
    ListRepos,
    ListProfiles,
    Submit,
}

pub(crate) struct Builtin(Op);

pub(crate) fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

pub(crate) fn root_prop() -> Value {
    json!({
        "type": "string",
        "description": "The plan's root ref. Defaults to the plan this session works on."
    })
}

/// The built-in tools, in listing order: the graph's, then plan review's
/// ([`crate::review`]).
pub fn builtin() -> Vec<ToolSpec> {
    let mut tools = graph();
    tools.extend(crate::review::tools());
    tools
}

fn graph() -> Vec<ToolSpec> {
    let t = |op, name: &str, title: &str, description: &str, schema| {
        ToolSpec::new(name, description, schema, Builtin(op)).title(title)
    };
    vec![
        t(
            Op::PlanRead,
            "plan_read",
            "Read a plan",
            "The plan's outline: every item with its parent, kind, title, phase and current \
             conditions (Ready, Blocked, Running, …), the edges between items, and the plan's \
             revision, approved revision and the items that drifted from it. The text is \
             markdown to show the person: a header, a Mermaid graph, and a table of the items. \
             The graph is left out, or collapsed by phase, when it's too wide to read (more \
             than 4 items side by side) or too big (over 40 items); pass `under` to read one \
             item's subtree instead.",
            object(
                json!({
                    "root": root_prop(),
                    "under": {
                        "type": "string",
                        "description": "An item's ref: read only the items below it."
                    }
                }),
                &[],
            ),
        )
        .read_only(),
        t(
            Op::ItemRead,
            "item_read",
            "Read an item",
            "One item in full: its spec and payload, status and labels, its annotations \
             (learnings, notes), and its runs with their steps and artifacts.",
            object(
                json!({ "ref": { "type": "string", "description": "The item's ref." } }),
                &["ref"],
            ),
        )
        .read_only(),
        t(
            Op::Ready,
            "ready",
            "Ready items",
            "The items that can start now: approved, every blocker done, no live run, and no \
             unapproved or drifted ancestor.",
            object(json!({ "root": root_prop() }), &[]),
        )
        .read_only(),
        t(
            Op::AddItem,
            "add_item",
            "Add an item",
            "Add an item under `parent`. It lands proposed: nothing runs it until the plan's \
             revision is approved. `kind` is a node kind's short name (`Task`, `Gate`, \
             `Decision`) or its type URL; `payload` is that kind's message as proto-JSON. \
             Returns the new item.",
            object(
                json!({
                    "parent": { "type": "string", "description": "The parent item's ref." },
                    "kind": { "type": "string" },
                    "title": { "type": "string" },
                    "body": { "type": "string", "description": "Markdown." },
                    "payload": { "type": "object", "description": "The kind's message as proto-JSON; `@type` may be left out." },
                    "labels": { "type": "object", "additionalProperties": { "type": "string" } }
                }),
                &["parent", "kind", "title"],
            ),
        ),
        t(
            Op::UpdateItem,
            "update_item",
            "Update an item",
            "Change an item that already exists. Only the fields given change: `title`, \
             `body`, `payload` (the kind's message as proto-JSON; it replaces the payload \
             whole) and `labels` (they replace the item's labels whole). The kind can't change. \
             Pass `etag`, from `item_read`, to refuse the update if the item changed since. \
             While the plan executes, the change is assessed like any other amendment: most go \
             ahead, some wait for a person. Returns the updated item, as `item_read` does.",
            object(
                json!({
                    "ref": { "type": "string", "description": "The item's ref." },
                    "title": { "type": "string" },
                    "body": { "type": "string", "description": "Markdown." },
                    "payload": { "type": "object", "description": "The kind's message as proto-JSON; `@type` may be left out. Replaces the payload whole." },
                    "labels": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Replaces the item's labels whole." },
                    "etag": { "type": "string", "description": "The item's etag the change is based on (`metadata.etag` in `item_read`)." }
                }),
                &["ref"],
            ),
        ),
        t(
            Op::AddEdge,
            "add_edge",
            "Add an edge",
            "Join two items with an edge of a registered kind (`blocked_by`: `from` waits for \
             `to`; `relates_to`; `discovered_from`).",
            object(
                json!({
                    "kind": { "type": "string" },
                    "from": { "type": "string" },
                    "to": { "type": "string" }
                }),
                &["kind", "from", "to"],
            ),
        ),
        t(
            Op::Annotate,
            "annotate",
            "Annotate an item",
            "Attach a record to an item, outside the plan's content (not part of its revision): \
             a `Learning` ({lesson, evidence}), a `ReconcileNote` ({text}), a `ReviewVerdict` \
             ({run, step, pass, findings}), or another annotation type by type URL.",
            object(
                json!({
                    "node": { "type": "string" },
                    "kind": { "type": "string", "description": "`Learning`, `ReconcileNote`, `ReviewVerdict`, `DecisionOutcome`, or a type URL." },
                    "payload": { "type": "object", "description": "The annotation's message as proto-JSON." }
                }),
                &["node", "kind"],
            ),
        ),
        t(
            Op::Propose,
            "propose",
            "Propose a change",
            "Propose a change to the plan that someone else decides on, instead of making it: \
             what to change and why. A person is asked to accept or decline it; the outcome \
             shows under `attention` in `item_read`. Changes nothing in the plan.",
            object(
                json!({
                    "node": { "type": "string", "description": "The item the change concerns." },
                    "summary": { "type": "string", "description": "What to change and why (Markdown)." },
                    "changes": { "description": "The change itself, structured, if it helps (items to add, fields to edit)." }
                }),
                &["node", "summary"],
            ),
        ),
        t(
            Op::ReportChange,
            "report_change",
            "Report a change",
            "Report the PRs a run opened as one change, in landing order (the first lands \
             first), and any releases. A PR already reported, or detected, is updated by URL. \
             Use it after opening several PRs that land together (a stack): one call with all \
             of them.",
            object(
                json!({
                    "run": { "type": "string", "description": "The run's ref. Defaults to this session's run." },
                    "group": { "type": "string", "description": "A name for the group (a stack's name). Defaults to the run's ref." },
                    "prs": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "url": { "type": "string" },
                                "repo": { "type": "string" },
                                "number": { "type": "integer" },
                                "title": { "type": "string" },
                                "head": { "type": "string" },
                                "base": { "type": "string" },
                                "state": { "type": "string", "enum": ["open", "closed", "merged"] },
                                "draft": { "type": "boolean" }
                            },
                            "required": ["url"]
                        }
                    },
                    "releases": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "repo": { "type": "string" },
                                "package": { "type": "string" },
                                "version": { "type": "string" },
                                "url": { "type": "string" }
                            },
                            "required": ["package", "version"]
                        }
                    }
                }),
                &[],
            ),
        ),
        t(
            Op::ListRepos,
            "list_repos",
            "List repositories",
            "The repositories work can target, from the factory's registry.",
            object(json!({}), &[]),
        )
        .read_only(),
        t(
            Op::ListProfiles,
            "list_profiles",
            "List profiles",
            "The session profiles items can name, from the factory's registry.",
            object(json!({}), &[]),
        )
        .read_only(),
        t(
            Op::Submit,
            "submit",
            "Submit a plan",
            "Hand the plan in for approval at its current revision, with a note for the \
             reviewer. Returns the revision; a person approves it, never the agent.",
            object(
                json!({
                    "root": root_prop(),
                    "note": { "type": "string", "description": "What the reviewer should know (Markdown)." }
                }),
                &[],
            ),
        ),
    ]
}

pub(crate) fn str_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    opt_str(args, name).ok_or_else(|| Error::invalid(format!("`{name}` is required")))
}

pub(crate) fn opt_str<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

pub(crate) fn to_json(value: &impl serde::Serialize) -> Result<Value> {
    Ok(serde_json::to_value(value)?)
}

fn from_json<T: serde::de::DeserializeOwned>(what: &str, value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| Error::invalid(format!("{what}: {e}")))
}

/// `payload` as an `Any` of `type_url`, filling in `@type` when left out.
fn any(type_url: &str, payload: Option<&Value>) -> Result<Option<Any>> {
    let Some(payload) = payload else {
        return Ok(None);
    };
    let Value::Object(map) = payload else {
        return Err(Error::invalid("`payload` must be an object"));
    };
    let mut map: Map<String, Value> = map.clone();
    match map.get("@type").and_then(Value::as_str) {
        Some(t) if t != type_url => {
            return Err(Error::invalid(format!(
                "the payload is a {t}, the kind is {type_url}"
            )));
        }
        Some(_) => {}
        None => {
            map.insert("@type".into(), type_url.into());
        }
    }
    from_json("payload", Value::Object(map)).map(Some)
}

impl Builtin {
    /// The graph a call works on: `root` if given, else the caller's. A
    /// caller bound to a graph can't name another.
    pub(crate) fn root<'a>(call: &'a ToolCall<'_>, given: Option<&'a str>) -> Result<&'a str> {
        match (given, call.caller.root.as_deref()) {
            (Some(g), Some(bound)) if g != bound => Err(Error::precondition(format!(
                "this session works on {bound}, not {g}"
            ))),
            (Some(g), _) => Ok(g),
            (None, Some(bound)) => Ok(bound),
            (None, None) => Err(Error::invalid("`root` is required")),
        }
    }

    /// Check that the caller may write at `node_ref`, and return its graph.
    pub(crate) async fn writable(call: &ToolCall<'_>, node_ref: &str) -> Result<String> {
        let root = call.factory.root_of(node_ref).await?;
        if let Some(bound) = call.caller.root.as_deref()
            && bound != root
        {
            return Err(Error::precondition(format!(
                "{node_ref} isn't in the plan this session works on ({bound})"
            )));
        }
        // TODO(cedar): amendment scope. An executor's session may write only
        // inside its run node's subtree (`call.caller.node`); the control
        // plane asks the PDP with the caller, the action and the target
        // (docs/factory/work-graph.md § Steering). Until then the graph is
        // the only boundary, and every write records its run in the
        // revision's provenance.
        Ok(root)
    }

    fn node_kind(call: &ToolCall<'_>, kind: &str) -> Result<String> {
        let kinds = &call.factory.schema().proto().node_kinds;
        kinds
            .iter()
            .find(|k| k.kind == kind)
            .or_else(|| {
                kinds
                    .iter()
                    .find(|k| short_kind(&k.kind).eq_ignore_ascii_case(kind))
            })
            .map(|k| k.kind.clone())
            .ok_or_else(|| {
                let known: Vec<&str> = kinds.iter().map(|k| short_kind(&k.kind)).collect();
                Error::invalid(format!("unknown kind {kind:?}; one of {known:?}"))
            })
    }

    fn annotation_kind(kind: &str) -> String {
        if kind.contains('/') {
            return kind.into();
        }
        [
            LEARNING,
            RECONCILE_NOTE,
            schema::DECISION_OUTCOME,
            wg::ReviewVerdict::TYPE_URL,
        ]
        .into_iter()
        .find(|k| short_kind(k).eq_ignore_ascii_case(kind))
        .unwrap_or(kind)
        .into()
    }

    async fn note(call: &ToolCall<'_>, node: &str, text: String) -> Result<wg::Annotation> {
        let note = payload::pack(&wg::ReconcileNote {
            text,
            ..Default::default()
        });
        call.factory
            .annotate(node, RECONCILE_NOTE, Some(note), &call.caller.provenance())
            .await
    }

    async fn plan_read(call: ToolCall<'_>) -> Result<ToolOutput> {
        let root = Self::root(&call, opt_str(&call.arguments, "root"))?;
        let under = opt_str(&call.arguments, "under");
        let state = call.factory.graph(root).await?;
        let idx = Indexed::new(&state.graph);
        if let Some(u) = under
            && !idx.nodes.contains_key(u)
        {
            return Err(Error::NotFound(format!("{u} in the plan {root}")));
        }
        // With `under`, the item and what's below it, and the edges that
        // touch them.
        let scope: Option<std::collections::HashSet<&str>> = under.map(|u| {
            let mut set = std::collections::HashSet::from([u]);
            let mut stack = vec![u];
            while let Some(r) = stack.pop() {
                for c in idx.children.get(r).into_iter().flatten() {
                    if set.insert(c) {
                        stack.push(c);
                    }
                }
            }
            set
        });
        let in_scope = |r: &str| scope.as_ref().is_none_or(|s| s.contains(r));
        let nodes: Vec<Value> = state
            .graph
            .nodes
            .iter()
            .filter(|n| in_scope(&n.r#ref))
            .map(|n| {
                json!({
                    "ref": n.r#ref,
                    "parent": idx.parent.get(n.r#ref.as_str()),
                    "kind": short_kind(&n.kind),
                    "title": n.spec.title,
                    "phase": phase(n),
                    "conditions": holds(n).collect::<Vec<_>>(),
                })
            })
            .collect();
        let edges: Vec<Value> = state
            .graph
            .edges
            .iter()
            .filter(|e| e.kind != schema::CHILD_OF)
            .filter(|e| in_scope(&e.from) || in_scope(&e.to))
            .map(|e| json!({ "kind": e.kind, "from": e.from, "to": e.to }))
            .collect();
        let mut structured = json!({
            "root": root,
            "revision": state.revision,
            "approved_revision": state.approved,
            "drifted": state.drifted,
            "items": nodes,
            "edges": edges,
        });
        if let Some(u) = under {
            structured["under"] = u.into();
        }
        let text = render::plan(&state, under, call.client);
        Ok(ToolOutput::json(structured).with_text(text))
    }

    async fn item_read(call: ToolCall<'_>) -> Result<ToolOutput> {
        let node_ref = str_arg(&call.arguments, "ref")?;
        let root = call.factory.root_of(node_ref).await?;
        if let Some(bound) = call.caller.root.as_deref()
            && bound != root
        {
            return Err(Error::precondition(format!(
                "{node_ref} isn't in the plan this session works on ({bound})"
            )));
        }
        Self::item(&call, &root, node_ref).await
    }

    /// One item in full, as `item_read` returns it: markdown text, and the
    /// JSON in `structuredContent`.
    async fn item(call: &ToolCall<'_>, root: &str, node_ref: &str) -> Result<ToolOutput> {
        // Read from the graph, so the node carries its derived conditions.
        let state = call.factory.graph(root).await?;
        let node = state
            .graph
            .nodes
            .iter()
            .find(|n| n.r#ref == node_ref)
            .ok_or_else(|| Error::NotFound(node_ref.into()))?;
        let parent = Indexed::new(&state.graph)
            .parent
            .get(node_ref)
            .map(|p| (*p).to_owned());
        let annotations = call.factory.annotations(node_ref).await?;
        let runs: Vec<wg::Run> = call
            .factory
            .runs(root)
            .await?
            .into_iter()
            .filter(|r| r.node == node_ref)
            .collect();
        // Resolved ones too: that's how a proposal's outcome comes back.
        let attention: Vec<wg::Attention> = call
            .factory
            .attention(Some(root), true)
            .await?
            .into_iter()
            .filter(|a| a.node == node_ref)
            .collect();
        let text = render::item(
            &state,
            node,
            &render::ItemRecords {
                annotations: &annotations,
                runs: &runs,
                attention: &attention,
            },
        );
        Ok(ToolOutput::json(json!({
            "root": root,
            "parent": parent,
            "item": to_json(node)?,
            "annotations": to_json(&annotations)?,
            "runs": to_json(&runs)?,
            "attention": to_json(&attention)?,
        }))
        .with_text(text))
    }

    async fn ready(call: ToolCall<'_>) -> Result<ToolOutput> {
        let root = Self::root(&call, opt_str(&call.arguments, "root"))?;
        let state = call.factory.graph(root).await?;
        let ready: Vec<Value> = state
            .graph
            .nodes
            .iter()
            .filter(|n| holds(n).any(|c| c == condition::READY))
            .map(|n| json!({ "ref": n.r#ref, "kind": short_kind(&n.kind), "title": n.spec.title }))
            .collect();
        Ok(ToolOutput::json(json!({ "root": root, "ready": ready }))
            .with_text(render::ready(&state)))
    }

    async fn add_item(call: ToolCall<'_>) -> Result<ToolOutput> {
        let args = &call.arguments;
        let parent = str_arg(args, "parent")?;
        Self::writable(&call, parent).await?;
        let kind = Self::node_kind(&call, str_arg(args, "kind")?)?;
        let spec = wg::NodeSpec {
            title: str_arg(args, "title")?.into(),
            body: opt_str(args, "body").unwrap_or_default().into(),
            payload: any(&kind, args.get("payload"))?.into(),
            ..Default::default()
        };
        let labels: HashMap<String, String> = match args.get("labels") {
            Some(l) => from_json("labels", l.clone())?,
            None => HashMap::new(),
        };
        let node = call
            .factory
            .create_node(Some(parent), &kind, spec, labels, &call.caller.provenance())
            .await?;
        Ok(ToolOutput::json(json!({ "item": to_json(&node)? })))
    }

    async fn update_item(call: ToolCall<'_>) -> Result<ToolOutput> {
        let args = &call.arguments;
        let node_ref = str_arg(args, "ref")?;
        let root = Self::writable(&call, node_ref).await?;
        let current = call.factory.node(node_ref).await?;
        let mut spec = current.spec.clone().into_option().unwrap_or_default();
        let mut changed = false;
        if let Some(title) = args.get("title") {
            spec.title = title
                .as_str()
                .ok_or_else(|| Error::invalid("`title` must be a string"))?
                .into();
            changed = true;
        }
        if let Some(body) = args.get("body") {
            spec.body = body
                .as_str()
                .ok_or_else(|| Error::invalid("`body` must be a string"))?
                .into();
            changed = true;
        }
        if let Some(payload) = args.get("payload") {
            if let Some(t) = payload.get("@type").and_then(Value::as_str)
                && t != current.kind
            {
                return Err(Self::kind_change(&current.kind, t));
            }
            spec.payload = any(&current.kind, Some(payload))?.into();
            changed = true;
        }
        let labels: Option<HashMap<String, String>> = args
            .get("labels")
            .map(|l| from_json("labels", l.clone()))
            .transpose()?;
        if !changed && labels.is_none() {
            return Err(Error::invalid(
                "nothing to change: give `title`, `body`, `payload` or `labels`",
            ));
        }
        call.factory
            .update_node(
                node_ref,
                changed.then_some(spec),
                labels,
                opt_str(args, "etag").map(str::to_owned),
                &call.caller.provenance(),
            )
            .await?;
        Self::item(&call, &root, node_ref).await
    }

    fn kind_change(from: &str, to: &str) -> Error {
        Error::invalid(format!(
            "an item's kind can't change ({} to {}): add a new item instead",
            short_kind(from),
            short_kind(to)
        ))
    }

    async fn add_edge(call: ToolCall<'_>) -> Result<ToolOutput> {
        let args = &call.arguments;
        let edge = wg::Edge {
            kind: str_arg(args, "kind")?.into(),
            from: str_arg(args, "from")?.into(),
            to: str_arg(args, "to")?.into(),
            ..Default::default()
        };
        Self::writable(&call, &edge.from).await?;
        Self::writable(&call, &edge.to).await?;
        call.factory
            .add_edge(&edge, &call.caller.provenance())
            .await?;
        Ok(ToolOutput::json(
            json!({ "edge": { "kind": edge.kind, "from": edge.from, "to": edge.to } }),
        ))
    }

    async fn annotate(call: ToolCall<'_>) -> Result<ToolOutput> {
        let args = &call.arguments;
        let node = str_arg(args, "node")?;
        Self::writable(&call, node).await?;
        let kind = Self::annotation_kind(str_arg(args, "kind")?);
        let payload = any(&kind, args.get("payload"))?;
        let annotation = call
            .factory
            .annotate(node, &kind, payload, &call.caller.provenance())
            .await?;
        Ok(ToolOutput::json(
            json!({ "annotation": to_json(&annotation)? }),
        ))
    }

    async fn propose(call: ToolCall<'_>) -> Result<ToolOutput> {
        let args = &call.arguments;
        let node = str_arg(args, "node")?;
        let root = call.factory.root_of(node).await?;
        Self::root(&call, Some(&root))?;
        let summary = str_arg(args, "summary")?;
        let revision = call.factory.graph(&root).await?.revision;
        let mut text = summary.to_owned();
        if let Some(changes) = args.get("changes").filter(|c| !c.is_null()) {
            let pretty = serde_json::to_string_pretty(changes)?;
            text.push_str(&format!("\n\n```json\n{pretty}\n```"));
        }
        // TODO(cedar): whether the caller could have made the change itself
        // is the PDP's call; a proposal is always allowed.
        let item = call
            .factory
            .raise_attention(
                wg::Attention {
                    kind: wg::AttentionKind::ATTENTION_KIND_PROPOSAL.into(),
                    node: node.into(),
                    run: call.caller.run.clone().unwrap_or_default(),
                    summary: summary.lines().next().unwrap_or(summary).into(),
                    detail: payload::pack(&wg::ReconcileNote {
                        text,
                        ..Default::default()
                    })
                    .into(),
                    ..Default::default()
                },
                &call.caller.provenance(),
            )
            .await?;
        Ok(ToolOutput::json(json!({
            "status": "proposed",
            "node": node,
            "attention": item.r#ref,
        }))
        .with_text(render::proposed(
            node,
            &item.r#ref,
            &revision,
            &root,
            call.client,
        )))
    }

    async fn report_change(call: ToolCall<'_>) -> Result<ToolOutput> {
        let args = &call.arguments;
        let run = match (opt_str(args, "run"), call.caller.run.as_deref()) {
            (Some(r), Some(own)) if r != own => {
                return Err(Error::precondition(format!(
                    "this session reports for its own run ({own}), not {r}"
                )));
            }
            (Some(r), _) | (None, Some(r)) => r,
            (None, None) => return Err(Error::invalid("`run` is required")),
        };
        let (root, _) = call.factory.find_run(run).await?;
        Self::root(&call, Some(&root))?;
        let mut prs: Vec<wg::PullRequest> = Vec::new();
        for pr in args
            .get("prs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            prs.push(from_json("a PR", pr.clone())?);
        }
        // A named group is numbered here; an unnamed one of several PRs by
        // the factory, after the run.
        if let Some(name) = opt_str(args, "group") {
            let of = prs.len() as u32;
            for (i, pr) in prs.iter_mut().enumerate() {
                pr.group = wg::Group {
                    name: name.into(),
                    position: i as u32 + 1,
                    of,
                    ..Default::default()
                }
                .into();
            }
        }
        let mut artifacts: Vec<wg::Artifact> = prs
            .into_iter()
            .map(|pr| wg::Artifact {
                artifact: Some(wg::artifact::Artifact::PullRequest(Box::new(pr))),
                ..Default::default()
            })
            .collect();
        for release in args
            .get("releases")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let release: wg::Release = from_json("a release", release.clone())?;
            artifacts.push(wg::Artifact {
                artifact: Some(wg::artifact::Artifact::Release(Box::new(release))),
                ..Default::default()
            });
        }
        if artifacts.is_empty() {
            return Err(Error::invalid("report at least one PR or release"));
        }
        let run = call
            .factory
            .report_artifacts(run, &artifacts, &call.caller.provenance())
            .await?;
        Ok(ToolOutput::json(json!({
            "run": run.r#ref,
            "artifacts": to_json(&run.artifacts)?,
        })))
    }

    async fn submit(call: ToolCall<'_>) -> Result<ToolOutput> {
        let root = Self::root(&call, opt_str(&call.arguments, "root"))?.to_owned();
        Self::writable(&call, &root).await?;
        let state = call.factory.graph(&root).await?;
        let revision = state.revision.clone();
        if let Some(note) = opt_str(&call.arguments, "note") {
            Self::note(&call, &root, note.to_owned()).await?;
        }
        // A planning run (`plan.v1`) asks for the approval itself once its
        // session goes idle. Without one, or when a person is already asked,
        // nobody else will, so ask here.
        let planning = match call.caller.run.as_deref() {
            Some(run) => call
                .factory
                .run(&root, run)
                .await
                .is_ok_and(|r| r.executor == factory_core::executor::plan::NAME),
            None => false,
        };
        let asked = call
            .factory
            .attention(Some(&root), false)
            .await?
            .iter()
            .any(|a| {
                a.node == root
                    && a.kind.as_known() == Some(wg::AttentionKind::ATTENTION_KIND_APPROVAL)
            });
        let attention = if planning || asked {
            None
        } else {
            let item = call
                .factory
                .raise_attention(
                    wg::Attention {
                        kind: wg::AttentionKind::ATTENTION_KIND_APPROVAL.into(),
                        node: root.clone(),
                        summary: format!("Review the plan at revision {revision}"),
                        ..Default::default()
                    },
                    &call.caller.provenance(),
                )
                .await?;
            Some(item.r#ref)
        };
        let diff = crate::review::compact_diff(&call, &root, &state).await?;
        let text = render::submitted(&state, &diff, attention.as_deref(), call.client);
        Ok(ToolOutput::json(json!({
            "status": "awaiting_approval",
            "root": root,
            "revision": revision,
            "attention": attention,
        }))
        .with_text(text))
    }
}

#[async_trait]
impl ToolHandler for Builtin {
    async fn call(&self, call: ToolCall<'_>) -> Result<ToolOutput> {
        match self.0 {
            Op::PlanRead => Self::plan_read(call).await,
            Op::ItemRead => Self::item_read(call).await,
            Op::Ready => Self::ready(call).await,
            Op::AddItem => Self::add_item(call).await,
            Op::UpdateItem => Self::update_item(call).await,
            Op::AddEdge => Self::add_edge(call).await,
            Op::Annotate => Self::annotate(call).await,
            Op::Propose => Self::propose(call).await,
            Op::ReportChange => Self::report_change(call).await,
            Op::ListRepos => Ok(ToolOutput::json(json!({
                "repos": to_json(&call.factory.registry().repos())?
            }))),
            Op::ListProfiles => Ok(ToolOutput::json(json!({
                "profiles": to_json(&call.factory.registry().profiles())?
            }))),
            Op::Submit => Self::submit(call).await,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn builtin_names_match_the_list() {
        let names: Vec<String> = super::builtin().into_iter().map(|t| t.name).collect();
        assert_eq!(names, super::BUILTIN);
    }
}
