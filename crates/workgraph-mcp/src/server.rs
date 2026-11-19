//! The MCP wire: listing and calling tools for one caller.
//!
//! A client sees the tools its grant's policy allows. App-only tools
//! ([`Visibility::App`]) are listed, and callable, only for a client that
//! declared the MCP Apps extension, per request or session, so one HTTP
//! server can serve an agent and an MCP Apps host (the desktop's side
//! channel) at once. A tool a client may not use doesn't exist for it: it's
//! not listed, and calling it is an unknown tool.
//!
//! Results are text and `structuredContent`, never an embedded resource
//! (see [`factory_core::tool`]).

use std::collections::HashSet;
use std::sync::Arc;

use axum::http::request::Parts;
use factory_core::tool::{ToolCall, ToolOutput, ToolSpec, Visibility};
use factory_core::{Error as CoreError, Factory};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, JsonObject, ListToolsResult, MetaObject, PaginatedRequestParams,
    ServerCapabilities, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Value, json};

use crate::auth::Grant;
use crate::tools;

/// The MCP Apps extension id, as clients declare it in their capabilities.
pub const UI_EXTENSION: &str = "io.modelcontextprotocol/ui";

const INSTRUCTIONS: &str = "\
The factory's work graph: plans, their items, and the edges between them. Read a \
plan with `plan_read`, an item with `item_read`, and what can start with `ready`. \
Items you add land proposed and run only once the plan's revision is approved. \
Refs are opaque: copy them from earlier results, never build one.";

/// Building the server failed: an extension's tool clashes.
#[derive(Debug)]
pub struct ToolClash(pub String);

impl std::fmt::Display for ToolClash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "an extension's MCP tool {:?} clashes with a built-in tool",
            self.0
        )
    }
}

impl std::error::Error for ToolClash {}

/// The work-graph MCP server over one [`Factory`]. Cheap to clone.
#[derive(Clone)]
pub struct WorkgraphMcp {
    factory: Arc<Factory>,
    tools: Arc<[ToolSpec]>,
    /// The grant of a client that presents no token (stdio, one process per
    /// client). Over HTTP every request brings its own.
    fixed: Option<Arc<Grant>>,
}

impl std::fmt::Debug for WorkgraphMcp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.tools.iter().map(|t| t.name.as_str()).collect();
        f.debug_struct("WorkgraphMcp")
            .field("tools", &names)
            .finish_non_exhaustive()
    }
}

impl WorkgraphMcp {
    /// The built-in tools and every extension's (`factory.extensions()`),
    /// in that order. Fails when an extension reuses a built-in tool's name
    /// (extensions among themselves are checked by the `FactoryBuilder`).
    pub fn new(factory: Arc<Factory>) -> Result<Self, ToolClash> {
        let mut all = tools::builtin();
        let builtin: HashSet<&str> = tools::BUILTIN.into_iter().collect();
        for tool in factory.extensions().mcp_tools() {
            if builtin.contains(tool.name.as_str()) {
                return Err(ToolClash(tool.name.clone()));
            }
            all.push(tool.clone());
        }
        Ok(Self {
            factory,
            tools: all.into(),
            fixed: None,
        })
    }

    /// Serve every client as `grant`, without tokens (stdio).
    #[must_use]
    pub fn with_grant(mut self, grant: Grant) -> Self {
        self.fixed = Some(Arc::new(grant));
        self
    }

    pub fn factory(&self) -> &Arc<Factory> {
        &self.factory
    }

    /// Every tool, before any policy.
    pub fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// The tools `grant` may see, for a client with (`apps`) or without the
    /// MCP Apps extension.
    pub fn visible<'a>(
        &'a self,
        grant: &'a Grant,
        apps: bool,
    ) -> impl Iterator<Item = &'a ToolSpec> {
        self.tools
            .iter()
            .filter(move |t| apps || t.visibility != Visibility::App)
            .filter(move |t| grant.policy.allows(t))
    }

    fn grant(&self, context: &RequestContext<RoleServer>) -> Option<Grant> {
        if let Some(fixed) = &self.fixed {
            return Some((**fixed).clone());
        }
        context
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<Grant>())
            .cloned()
    }
}

fn meta(value: Value) -> MetaObject {
    match value {
        Value::Object(map) => MetaObject(map),
        _ => unreachable!("_meta is an object"),
    }
}

fn input_schema(value: &Value) -> Arc<JsonObject> {
    match value {
        Value::Object(map) => Arc::new(map.clone()),
        // A tool without a usable schema takes no arguments.
        _ => Arc::new(
            json!({ "type": "object" })
                .as_object()
                .cloned()
                .unwrap_or_default(),
        ),
    }
}

/// A tool as MCP lists it.
pub fn mcp_tool(spec: &ToolSpec) -> Tool {
    let annotations = ToolAnnotations::new()
        .read_only(spec.read_only)
        .destructive(false);
    let mut tool = Tool::new(
        spec.name.clone(),
        spec.description.clone(),
        input_schema(&spec.input_schema),
    )
    .with_title(spec.title.clone())
    .with_annotations(annotations);
    let visibility = match spec.visibility {
        Visibility::Both => None,
        Visibility::Model => Some(json!(["model"])),
        Visibility::App => Some(json!(["app"])),
    };
    let mut ui = serde_json::Map::new();
    if let Some(uri) = &spec.ui_resource {
        ui.insert("resourceUri".into(), uri.clone().into());
    }
    if let Some(v) = visibility {
        ui.insert("visibility".into(), v);
    }
    if !ui.is_empty() {
        tool = tool.with_meta(meta(json!({ "ui": ui })));
    }
    tool
}

/// A tool's output as an MCP result: its text, and `structuredContent` when
/// it has one. Never a resource.
pub fn tool_result(output: ToolOutput) -> CallToolResult {
    let content = vec![ContentBlock::text(output.text)];
    let mut result = if output.is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    };
    result.structured_content = output.structured;
    result
}

/// Whether the client behind this request declared the MCP Apps extension:
/// in the request's `_meta` (the stateless protocol), else in its session's
/// `initialize`.
fn supports_apps(context: &RequestContext<RoleServer>) -> bool {
    context
        .client_capabilities()
        .and_then(|capabilities| capabilities.extensions)
        .is_some_and(|extensions| extensions.contains_key(UI_EXTENSION))
}

fn no_grant() -> McpError {
    // The HTTP layer refuses requests without a token before they get here;
    // this is a server wired up without it.
    McpError::invalid_request("no grant for this request", None)
}

impl ServerHandler for WorkgraphMcp {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("workgraph", env!("CARGO_PKG_VERSION"))
                    .with_title("Factory work graph"),
            )
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let grant = self.grant(&context).ok_or_else(no_grant)?;
        let apps = supports_apps(&context);
        Ok(ListToolsResult::with_all_items(
            self.visible(&grant, apps).map(mcp_tool).collect(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let grant = self.grant(&context).ok_or_else(no_grant)?;
        let apps = supports_apps(&context);
        let Some(spec) = self
            .visible(&grant, apps)
            .find(|t| t.name == request.name.as_ref())
        else {
            return Err(McpError::invalid_params(
                format!("unknown tool `{}`", request.name),
                None,
            ));
        };
        let caller = grant.caller();
        let call = ToolCall {
            factory: &self.factory,
            caller: &caller,
            arguments: request
                .arguments
                .clone()
                .map(Value::Object)
                .unwrap_or_else(|| json!({})),
        };
        let output = match spec.handler.call(call).await {
            Ok(output) => output,
            Err(err) => {
                if matches!(err, CoreError::Internal(_)) {
                    tracing::warn!(tool = %spec.name, actor = %caller.actor, %err, "tool failed");
                }
                ToolOutput::error(err.to_string())
            }
        };
        tracing::debug!(
            tool = %spec.name,
            actor = %caller.actor,
            role = %caller.role,
            error = output.is_error,
            "tool called"
        );
        Ok(tool_result(output).into())
    }
}
