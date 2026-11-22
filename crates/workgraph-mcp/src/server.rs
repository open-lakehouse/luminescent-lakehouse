//! The MCP wire: listing and calling tools for one caller.
//!
//! A client sees the tools its grant's policy allows. App-only tools
//! ([`Visibility::App`]) and the MCP Apps' views (`ui://` resources) are
//! listed, and callable or readable, only for a client that declared the MCP
//! Apps extension, per request or session, *and* holds an apps grant
//! ([`Grant::apps`]), so one HTTP server can serve an agent and an MCP Apps
//! host (the desktop's side channel) at once, and no agent's token reaches
//! approval whatever its client declares. A tool a client may not use doesn't exist for it: it's
//! not listed, and calling it is an unknown tool.
//!
//! Results are text and `structuredContent`, never an embedded resource
//! (see [`factory_core::tool`]).

use std::collections::HashSet;
use std::sync::Arc;

use axum::http::request::Parts;
use factory_core::tool::{
    Client, MCP_APP_MIME_TYPE, ToolCall, ToolOutput, ToolSpec, UiResource, Visibility,
};
use factory_core::{Error as CoreError, Factory};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, JsonObject, ListResourcesResult, ListToolsResult, MetaObject,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    Resource, ResourceContents, ServerCapabilities, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Value, json};

use crate::auth::Grant;
use crate::{review, tools};

/// The MCP Apps extension id, as clients declare it in their capabilities.
pub const UI_EXTENSION: &str = "io.modelcontextprotocol/ui";

const INSTRUCTIONS: &str = "\
The factory's work graph: plans, their items, and the edges between them. Read a \
plan with `plan_read`, an item with `item_read`, and what can start with `ready`. \
Items you add land proposed and run only once the plan's revision is approved. \
Refs are opaque: copy them from earlier results, never build one.";

/// Building the server failed: an extension's tool, or view, clashes with a
/// built-in one (the name, or the `ui://` URI).
#[derive(Debug)]
pub struct ToolClash(pub String);

impl std::fmt::Display for ToolClash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "an extension's MCP tool or view {:?} clashes with a built-in one",
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
    resources: Arc<[UiResource]>,
    /// The grant of a client that presents no token (stdio, one process per
    /// client). Over HTTP every request brings its own.
    fixed: Option<Arc<Grant>>,
    /// The control room's base URL, linked from results for clients that
    /// can't show plan review.
    control_room: Option<Arc<str>>,
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
        let mut resources = vec![review::resource()];
        for resource in factory.extensions().ui_resources() {
            if resources.iter().any(|r| r.uri == resource.uri) {
                return Err(ToolClash(resource.uri.clone()));
            }
            resources.push(resource.clone());
        }
        Ok(Self {
            factory,
            tools: all.into(),
            resources: resources.into(),
            fixed: None,
            control_room: None,
        })
    }

    /// Serve every client as `grant`, without tokens (stdio).
    #[must_use]
    pub fn with_grant(mut self, grant: Grant) -> Self {
        self.fixed = Some(Arc::new(grant));
        self
    }

    /// Link results to the control room at `url` (its base URL; plans open
    /// at `?plan=<root>`), for clients that can't show plan review.
    #[must_use]
    pub fn with_control_room(mut self, url: impl Into<String>) -> Self {
        let url: String = url.into();
        self.control_room = (!url.is_empty()).then(|| url.into());
        self
    }

    pub fn factory(&self) -> &Arc<Factory> {
        &self.factory
    }

    /// Every tool, before any policy.
    pub fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// Every MCP App's view: plan review's, then the extensions'.
    pub fn resources(&self) -> &[UiResource] {
        &self.resources
    }

    /// The tools `grant` may see, for a client with (`apps`) or without the
    /// MCP Apps extension. App-only tools need both the client's declaration
    /// and an apps grant.
    pub fn visible<'a>(
        &'a self,
        grant: &'a Grant,
        apps: bool,
    ) -> impl Iterator<Item = &'a ToolSpec> {
        let apps = apps && grant.apps;
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

/// A view as MCP lists it.
pub fn mcp_resource(resource: &UiResource) -> Resource {
    let mut listed = Resource::new(resource.uri.clone(), resource.name.clone())
        .with_title(resource.title.clone())
        .with_mime_type(MCP_APP_MIME_TYPE);
    if !resource.description.is_empty() {
        listed = listed.with_description(resource.description.clone());
    }
    listed
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
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
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
        let client = Client {
            apps: apps && grant.apps,
            control_room: self.control_room.as_deref().map(str::to_owned),
        };
        let call = ToolCall {
            factory: &self.factory,
            caller: &caller,
            client: &client,
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

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let grant = self.grant(&context).ok_or_else(no_grant)?;
        if !(grant.apps && supports_apps(&context)) {
            return Ok(ListResourcesResult::with_all_items(Vec::new()));
        }
        Ok(ListResourcesResult::with_all_items(
            self.resources.iter().map(mcp_resource).collect(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let grant = self.grant(&context).ok_or_else(no_grant)?;
        let found = (grant.apps && supports_apps(&context))
            .then(|| self.resources.iter().find(|r| r.uri == request.uri))
            .flatten();
        let Some(resource) = found else {
            return Err(McpError::resource_not_found(
                format!("no resource `{}`", request.uri),
                None,
            ));
        };
        Ok(
            ReadResourceResult::new(vec![ResourceContents::TextResourceContents {
                uri: resource.uri.clone(),
                mime_type: Some(MCP_APP_MIME_TYPE.into()),
                text: resource.html.to_string(),
                meta: resource.meta().map(|ui| meta(json!({ "ui": ui }))),
            }])
            .into(),
        )
    }
}
