//! Plan review as an MCP App: a person approves a plan, or returns it with
//! notes, from a view the host renders next to the agent's call.
//!
//! - `open_plan_review` (read-only, for the model and the app) opens
//!   [`PLAN_VIEW`]. Its text tells the model that nothing is approved yet,
//!   and its `structuredContent` (`status: "awaiting_user"`) carries the
//!   plan's revision, the approved one, and a [`Snapshot`] of what the view
//!   renders, so the view needs no network.
//! - `approve_revision` and `return_plan` are app-only: listed and callable
//!   only for an MCP Apps host with an apps grant, never for an agent's
//!   token. The view calls them through the host, and reports the outcome to
//!   the model with `ui/update-model-context`.
//!
//! The view is `assets/plan-review.html`, the single-file build of
//! `packages/factory-plan-review`, committed and checked for drift.

use std::collections::BTreeMap;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use buffa::Message;
use factory_core::tool::{ToolCall, ToolHandler, ToolOutput, ToolSpec, UiResource};
use factory_core::{Error, Result, payload, schema::RECONCILE_NOTE, wire};
use factory_proto::v1;
use factory_proto::workgraph::v1 as wg;
use serde_json::json;

use crate::tools::{Builtin, object, opt_str, root_prop, str_arg};

/// The plan review's view.
pub const PLAN_VIEW: &str = "ui://factory/plan";

/// The view: one inline HTML document (`packages/factory-plan-review`'s
/// build, synced by its `scripts/sync.ts`).
pub const PLAN_VIEW_HTML: &str = include_str!("../assets/plan-review.html");

/// The names of the plan review's tools, among the built-in ones.
pub const TOOLS: [&str; 3] = ["open_plan_review", "approve_revision", "return_plan"];

/// The view as a resource.
pub fn resource() -> UiResource {
    UiResource::new(PLAN_VIEW, "plan-review", PLAN_VIEW_HTML)
        .title("Review a plan")
        .description(
            "The plan's work graph and its changes since the approved revision, with approve \
             and return (MCP App).",
        )
}

#[derive(Clone, Copy)]
enum Op {
    Open,
    Approve,
    Return,
}

struct Review(Op);

/// The plan review's tools, in listing order.
pub fn tools() -> Vec<ToolSpec> {
    let revision = json!({
        "type": "string",
        "description": "The revision (root hash) the person reviewed."
    });
    vec![
        ToolSpec::new(
            "open_plan_review",
            "Show the plan to the person for review: its items and edges, and what changed \
             since the approved revision. It approves nothing. The person approves the plan or \
             returns it with notes in the review, and the outcome arrives as an \
             `<mcp-app-context>` message; until then, don't treat the plan as approved.",
            object(json!({ "root": root_prop() }), &[]),
            Review(Op::Open),
        )
        .title("Review the plan")
        .read_only()
        .ui(PLAN_VIEW),
        ToolSpec::new(
            "approve_revision",
            "Approve the plan at the revision the person reviewed. Fails if the plan changed \
             since.",
            object(
                json!({ "root": root_prop(), "revision": revision.clone() }),
                &["revision"],
            ),
            Review(Op::Approve),
        )
        .title("Approve the plan")
        .app_only(),
        ToolSpec::new(
            "return_plan",
            "Return the plan to its planner with the person's notes, unapproved.",
            object(
                json!({
                    "root": root_prop(),
                    "revision": revision,
                    "notes": { "type": "string", "description": "What to change (Markdown)." }
                }),
                &["notes"],
            ),
            Review(Op::Return),
        )
        .title("Return the plan")
        .app_only(),
    ]
}

/// What the view renders, as factory.v1 messages in the binary format
/// (base64), the answers of the RPCs factory-ui's components ask: binary, so
/// an extension's payloads pass through intact without a registry entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    /// `GetSchemaResponse`.
    pub schema: String,
    /// `GetGraphResponse`.
    pub graph: String,
    /// `GetRevisionResponse` of the revision under review.
    pub target: String,
    /// `GetRevisionResponse` of the approved revision; empty if none is.
    pub base: String,
    /// `DiffRevisionsResponse`, approved → target, compact (review level).
    pub diff: String,
    /// The same, every entry.
    pub diff_full: String,
}

fn b64(m: &impl Message) -> String {
    BASE64.encode(m.encode_to_vec())
}

/// The open approval items of a plan.
async fn open_approvals(call: &ToolCall<'_>, root: &str) -> Result<Vec<wg::Attention>> {
    Ok(call
        .factory
        .attention(Some(root), false)
        .await?
        .into_iter()
        .filter(|a| {
            a.node == root && a.kind.as_known() == Some(wg::AttentionKind::ATTENTION_KIND_APPROVAL)
        })
        .collect())
}

fn title(state: &factory_core::GraphState, root: &str) -> String {
    state
        .graph
        .nodes
        .iter()
        .find(|n| n.r#ref == root)
        .map(|n| n.spec.title.clone())
        .unwrap_or_default()
}

impl Review {
    async fn open(call: ToolCall<'_>) -> Result<ToolOutput> {
        let root = Builtin::root(&call, opt_str(&call.arguments, "root"))?.to_owned();
        let state = call.factory.graph(&root).await?;
        let approved = state.approved.clone();
        let revision = state.revision.clone();
        let target = call.factory.revision(&root, Some(&revision)).await?;
        let base = match approved.as_deref() {
            Some(a) => Some(call.factory.revision(&root, Some(a)).await?),
            None => None,
        };
        let compact = call
            .factory
            .diff(&root, approved.as_deref(), Some(&revision), true)
            .await?;
        let full = call
            .factory
            .diff(&root, approved.as_deref(), Some(&revision), false)
            .await?;
        let mut changes: BTreeMap<&'static str, u32> = BTreeMap::new();
        for e in &compact {
            let name = match e.change {
                plan_tree::Change::Added => "added",
                plan_tree::Change::Removed => "removed",
                plan_tree::Change::Modified => "modified",
                plan_tree::Change::ModifiedDescendants => "modified_descendants",
                plan_tree::Change::Moved => "moved",
            };
            *changes.entry(name).or_default() += 1;
        }
        let entries = |e: Vec<plan_tree::DiffEntry>| v1::DiffRevisionsResponse {
            entries: e.into_iter().map(wire::diff_entry).collect(),
            ..Default::default()
        };
        let title = title(&state, &root);
        let snapshot = Snapshot {
            schema: b64(&wire::schema_response(call.factory.extensions())),
            graph: b64(&wire::graph_response(state)),
            target: b64(&wire::revision_response(&target, approved.as_deref())),
            base: base
                .as_ref()
                .map(|b| b64(&wire::revision_response(b, approved.as_deref())))
                .unwrap_or_default(),
            diff: b64(&entries(compact)),
            diff_full: b64(&entries(full)),
        };
        let since = match approved.as_deref() {
            Some(a) if a == revision => "unchanged since its approval".to_owned(),
            Some(a) => format!("compared with the approved revision {a}"),
            None => "never approved before".to_owned(),
        };
        let text = format!(
            "Opened the review of the plan {title:?} ({root}) at revision {revision}, {since}. \
             Nothing is approved until the person decides; the outcome arrives as an \
             `<mcp-app-context>` message."
        );
        Ok(ToolOutput::json(json!({
            "status": "awaiting_user",
            "root": root,
            "title": title,
            "revision": revision,
            "approved_revision": approved,
            "changes": changes,
            "snapshot": serde_json::to_value(&snapshot)?,
        }))
        .with_text(text))
    }

    async fn approve(call: ToolCall<'_>) -> Result<ToolOutput> {
        let root = Builtin::root(&call, opt_str(&call.arguments, "root"))?.to_owned();
        Builtin::writable(&call, &root).await?;
        let revision = str_arg(&call.arguments, "revision")?;
        let provenance = call.caller.provenance();
        let approved = call.factory.approve(&root, revision, &provenance).await?;
        // Approving answers the open approval items; the reconciler sees them
        // accepted at the approved revision and has nothing left to do.
        for item in open_approvals(&call, &root).await? {
            call.factory
                .resolve_attention(
                    &item.r#ref,
                    "accept",
                    "Approved in the plan review",
                    &provenance,
                )
                .await?;
        }
        Ok(ToolOutput::json(json!({
            "status": "approved",
            "root": root,
            "revision": approved.root_hash,
        }))
        .with_text(format!(
            "Approved the plan {root} at revision {}.",
            approved.root_hash
        )))
    }

    async fn return_plan(call: ToolCall<'_>) -> Result<ToolOutput> {
        let root = Builtin::root(&call, opt_str(&call.arguments, "root"))?.to_owned();
        Builtin::writable(&call, &root).await?;
        let notes = str_arg(&call.arguments, "notes")
            .map_err(|_| Error::invalid("say what to change: `notes` is required"))?
            .to_owned();
        let state = call.factory.graph(&root).await?;
        let revision = opt_str(&call.arguments, "revision").unwrap_or(&state.revision);
        let provenance = call.caller.provenance();
        let note = payload::pack(&wg::ReconcileNote {
            text: format!("Returned at revision {revision}:\n\n{notes}"),
            ..Default::default()
        });
        call.factory
            .annotate(&root, RECONCILE_NOTE, Some(note), &provenance)
            .await?;
        for item in open_approvals(&call, &root).await? {
            call.factory
                .resolve_attention(&item.r#ref, "decline", &notes, &provenance)
                .await?;
        }
        Ok(ToolOutput::json(json!({
            "status": "returned",
            "root": root,
            "revision": revision,
            "notes": notes,
        }))
        .with_text(format!("Returned the plan {root} with notes: {notes}")))
    }
}

#[async_trait]
impl ToolHandler for Review {
    async fn call(&self, call: ToolCall<'_>) -> Result<ToolOutput> {
        match self.0 {
            Op::Open => Self::open(call).await,
            Op::Approve => Self::approve(call).await,
            Op::Return => Self::return_plan(call).await,
        }
    }
}
