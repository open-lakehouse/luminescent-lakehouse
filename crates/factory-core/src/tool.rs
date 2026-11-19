//! Tools for the agents' MCP server (`workgraph-mcp`): what an extension
//! contributes through [`Extension::mcp_tools`](crate::Extension::mcp_tools),
//! and what the built-in tools are made of too.
//!
//! A tool is a name, a JSON Schema for its arguments, a few hints, and a
//! [`ToolHandler`]. It doesn't say who may call it: the token a client
//! presents carries that policy, which the composition chooses.
//!
//! A tool returns text and, optionally, structured JSON ([`ToolOutput`]),
//! and nothing else. In particular it can't return an embedded resource:
//! agent hosts mishandle them (Omnigent 0.21 and 0.22 fail serializing one),
//! and an MCP App's view is read by the host as a `ui://` resource instead.
//!
//! That view is a [`UiResource`], which an extension contributes through
//! [`Extension::ui_resources`](crate::Extension::ui_resources): one HTML
//! document, served only to MCP Apps hosts.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::{Factory, Provenance, Result};

/// The `via` of every write made through a tool.
pub const VIA: &str = "workgraph-mcp";

/// Who a tool may be shown to, after the caller's policy allows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// The model (agents) and the app.
    #[default]
    Both,
    /// The model only: an MCP App's view can't call it.
    Model,
    /// An MCP App's view only. Listed and callable only for clients that
    /// declare the MCP Apps extension, and hidden from the model by the host
    /// (`_meta.ui.visibility: ["app"]`). Approval-style writes belong here,
    /// so an agent can't approve its own work.
    App,
}

/// The client a call comes from, as its token's grant names it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Caller {
    /// Who writes are attributed to (an agent role's service account, or a
    /// person).
    pub actor: String,
    /// A label the composition chose (`planner`, `reviewer`, …), for
    /// provenance and logs. The framework never branches on it.
    pub role: String,
    /// The graph the caller is bound to, if any. Tools default `root` to it,
    /// and refuse writes to other graphs.
    pub root: Option<String>,
    /// The run the caller's session belongs to, if any.
    pub run: Option<String>,
    /// The node the caller works on, if any: the subtree an amendment scope
    /// will be checked against (Cedar, later).
    pub node: Option<String>,
}

impl Caller {
    /// The provenance of a write made by this caller through a tool.
    pub fn provenance(&self) -> Provenance {
        let mut p = Provenance::new(self.actor.clone(), VIA);
        p.run_ref = self.run.clone().unwrap_or_default();
        p
    }
}

/// One call of a tool.
pub struct ToolCall<'a> {
    pub factory: &'a Factory,
    pub caller: &'a Caller,
    /// The arguments, a JSON object (`{}` when the client sent none).
    pub arguments: Value,
}

/// A tool's result: text for the model, and optionally structured JSON for
/// clients that read `structuredContent`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolOutput {
    pub text: String,
    pub structured: Option<Value>,
    /// A tool-level failure the model should see and may recover from.
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }

    /// `structured` as the result, and its JSON as the text.
    pub fn json(structured: Value) -> Self {
        Self {
            text: structured.to_string(),
            structured: Some(structured),
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: true,
        }
    }

    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }
}

/// What a tool does. An `Err` reaches the model as an error result with the
/// error's text.
#[async_trait]
pub trait ToolHandler: Send + Sync {
    async fn call(&self, call: ToolCall<'_>) -> Result<ToolOutput>;
}

/// A tool for `workgraph-mcp`.
#[derive(Clone)]
pub struct ToolSpec {
    pub name: String,
    pub title: String,
    pub description: String,
    /// The JSON Schema of the arguments (an object schema).
    pub input_schema: Value,
    /// Changes nothing. Policies can select read-only tools as a class
    /// (`@read`), and hosts may re-run them (an MCP App's side channel).
    pub read_only: bool,
    pub visibility: Visibility,
    /// An MCP App's `ui://` view the tool opens (`_meta.ui.resourceUri`).
    pub ui_resource: Option<String>,
    pub handler: Arc<dyn ToolHandler>,
}

impl fmt::Debug for ToolSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolSpec")
            .field("name", &self.name)
            .field("read_only", &self.read_only)
            .field("visibility", &self.visibility)
            .field("ui_resource", &self.ui_resource)
            .finish_non_exhaustive()
    }
}

impl ToolSpec {
    /// A tool that may write, visible to the model and the app.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl ToolHandler + 'static,
    ) -> Self {
        let name = name.into();
        Self {
            title: name.clone(),
            name,
            description: description.into(),
            input_schema,
            read_only: false,
            visibility: Visibility::Both,
            ui_resource: None,
            handler: Arc::new(handler),
        }
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    #[must_use]
    pub fn visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Only for an MCP App's view ([`Visibility::App`]).
    #[must_use]
    pub fn app_only(self) -> Self {
        self.visibility(Visibility::App)
    }

    /// The tool opens the MCP App at `uri` (`ui://…`).
    #[must_use]
    pub fn ui(mut self, uri: impl Into<String>) -> Self {
        self.ui_resource = Some(uri.into());
        self
    }
}

/// The MIME type of an MCP App's view.
pub const MCP_APP_MIME_TYPE: &str = "text/html;profile=mcp-app";

/// An MCP App's view: a `ui://` resource that a read-only tool opens
/// ([`ToolSpec::ui`]) and an MCP Apps host renders in its sandbox.
///
/// The view is **one HTML document**: scripts and styles inline (a
/// single-file build), no network, and no credentials. It gets its data from
/// the tool's result (`ui/notifications/tool-result`), acts through the
/// host (`tools/call`, the extension's app-only tools), and reports the
/// outcome to the model with `ui/update-model-context`. `workgraph-mcp`
/// serves it only to clients that declared the MCP Apps extension with an
/// apps grant, and never embeds it in a tool result.
#[derive(Debug, Clone)]
pub struct UiResource {
    /// `ui://<extension>/<view>`.
    pub uri: String,
    pub name: String,
    pub title: String,
    pub description: String,
    /// The document, usually an `include_str!` of a committed build.
    pub html: Cow<'static, str>,
    /// `_meta.ui.csp`, for a view that needs more than the host's default
    /// (none: the default is no network at all).
    pub csp: Option<Value>,
    /// `_meta.ui.prefersBorder`.
    pub prefers_border: Option<bool>,
}

impl UiResource {
    pub fn new(
        uri: impl Into<String>,
        name: impl Into<String>,
        html: impl Into<Cow<'static, str>>,
    ) -> Self {
        let name = name.into();
        Self {
            uri: uri.into(),
            title: name.clone(),
            name,
            description: String::new(),
            html: html.into(),
            csp: None,
            prefers_border: None,
        }
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    #[must_use]
    pub fn prefers_border(mut self, prefers: bool) -> Self {
        self.prefers_border = Some(prefers);
        self
    }

    /// `_meta.ui` of the resource, if it has any.
    pub fn meta(&self) -> Option<Value> {
        let mut ui = serde_json::Map::new();
        if let Some(csp) = &self.csp {
            ui.insert("csp".into(), csp.clone());
        }
        if let Some(b) = self.prefers_border {
            ui.insert("prefersBorder".into(), b.into());
        }
        (!ui.is_empty()).then_some(Value::Object(ui))
    }
}
