//! The events of a session's stream (`omnigent/server/schemas.py`, the `*Event` models), and
//! the items of its history.
//!
//! The parse is lenient, as the Python SDK's (`omnigent_client/_sse.py`) is: a missing field
//! reads as empty, and an event type this crate doesn't know is passed through as
//! [`Event::Unknown`].

use serde_json::{Map, Value};

use crate::api::ConversationItem;
use crate::from_struct;

/// The status a `response.*` lifecycle event reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseStatus {
    /// `response.created`: a turn started.
    Created,
    /// `response.queued`.
    Queued,
    /// `response.in_progress`.
    InProgress,
    /// `response.completed`.
    Completed,
    /// `response.failed`.
    Failed,
    /// `response.cancelled`: interrupted.
    Cancelled,
    /// `response.incomplete`.
    Incomplete,
}

impl ResponseStatus {
    /// Whether the turn is over.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Incomplete
        )
    }

    fn from_wire(kind: &str) -> Option<Self> {
        Some(match kind {
            "response.created" => Self::Created,
            "response.queued" => Self::Queued,
            "response.in_progress" => Self::InProgress,
            "response.completed" => Self::Completed,
            "response.failed" => Self::Failed,
            "response.cancelled" => Self::Cancelled,
            "response.incomplete" => Self::Incomplete,
            _ => return None,
        })
    }
}

/// Who wrote a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// The user.
    User,
    /// The agent.
    Assistant,
    /// The system (a developer or system message in the history).
    System,
}

impl Role {
    fn from_wire(role: &str) -> Self {
        match role {
            "user" => Self::User,
            "system" | "developer" => Self::System,
            _ => Self::Assistant,
        }
    }
}

/// A server-initiated elicitation (`response.elicitation_request`): a policy's approval
/// prompt, or an MCP server's `elicitation/create` form.
#[derive(Clone, Debug, PartialEq)]
pub struct ElicitationRequest {
    /// The id to resolve it with.
    pub elicitation_id: String,
    /// `form` or `url`.
    pub mode: String,
    /// The prompt.
    pub message: String,
    /// The form's JSON Schema (`requestedSchema`), in form mode.
    pub requested_schema: Option<Value>,
    /// The policy-engine phase, e.g. `pre_tool_use`; `None` for an MCP server's form.
    pub phase: Option<String>,
    /// The policy that asks.
    pub policy_name: Option<String>,
    /// A preview of what is asked about (a tool call's arguments).
    pub content_preview: Option<String>,
    /// The whole `params` object.
    pub params: Map<String, Value>,
}

impl ElicitationRequest {
    /// Read the event's payload (`{elicitation_id, params}`), as the stream sends it and as
    /// a snapshot's `pending_elicitations` keep it.
    pub fn from_json(data: &Map<String, Value>) -> Self {
        let params = data
            .get("params")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        Self {
            elicitation_id: string(data, "elicitation_id"),
            mode: opt_string(&params, "mode").unwrap_or_else(|| "form".to_owned()),
            message: string(&params, "message"),
            requested_schema: params
                .get("requestedSchema")
                .filter(|v| !v.is_null())
                .cloned(),
            phase: opt_string(&params, "phase"),
            policy_name: opt_string(&params, "policy_name"),
            content_preview: opt_string(&params, "content_preview"),
            params,
        }
    }
}

/// A completed item: of a turn (`response.output_item.done`), or of the history.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputItem {
    /// A message.
    Message {
        /// The item id.
        id: String,
        /// The response (turn) it belongs to.
        response_id: Option<String>,
        /// Its author.
        role: Role,
        /// Its text: the `output_text`/`input_text`/`text` blocks, joined.
        text: String,
        /// The content blocks as sent.
        content: Vec<Value>,
    },
    /// A tool call.
    FunctionCall {
        /// The item id.
        id: String,
        /// The response it belongs to.
        response_id: Option<String>,
        /// The id its output answers.
        call_id: String,
        /// The tool, as the agent sees it: `<server>__<tool>` for an MCP server's tool.
        name: String,
        /// The arguments, a JSON string.
        arguments: String,
        /// `completed`, `in_progress`, `action_required` (a client tool), `failed`, ...
        status: String,
    },
    /// A tool call's result.
    FunctionCallOutput {
        /// The item id.
        id: String,
        /// The response it belongs to.
        response_id: Option<String>,
        /// The call it answers.
        call_id: String,
        /// The result, as the agent sees it (a string; MCP results are flattened).
        output: String,
        /// The item status: `failed` for a tool error.
        status: String,
    },
    /// The agent's reasoning.
    Reasoning {
        /// The item id.
        id: String,
        /// The response it belongs to.
        response_id: Option<String>,
        /// The reasoning text, or its summary when the text is withheld.
        text: String,
    },
    /// An error recorded in the history.
    Error {
        /// The item id.
        id: String,
        /// The response it belongs to.
        response_id: Option<String>,
        /// `llm`, `execution`, `tool` or `harness`.
        source: String,
        /// The message.
        message: String,
    },
    /// Any other item (compaction, native tools, ...), as sent.
    Other {
        /// The item type.
        kind: String,
        /// The item.
        item: Map<String, Value>,
    },
}

impl OutputItem {
    /// The item's id.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Message { id, .. }
            | Self::FunctionCall { id, .. }
            | Self::FunctionCallOutput { id, .. }
            | Self::Reasoning { id, .. }
            | Self::Error { id, .. } => Some(id),
            Self::Other { item, .. } => item.get("id").and_then(Value::as_str),
        }
    }

    /// Read a stream item: the common fields (`id`, `type`, `status`, `response_id`) and
    /// the type's fields at the same level.
    pub fn from_stream(item: &Map<String, Value>) -> Self {
        let kind = string(item, "type");
        Self::parse(&kind, item, item)
    }

    /// Read a history item, whose type's fields are nested under `data`.
    pub fn from_conversation_item(item: &ConversationItem) -> Self {
        let mut common = Map::new();
        common.insert("id".into(), Value::String(item.id.clone()));
        common.insert("type".into(), Value::String(item.r#type.clone()));
        common.insert("status".into(), Value::String(item.status.clone()));
        common.insert(
            "response_id".into(),
            Value::String(item.response_id.clone()),
        );
        let data = item.data.as_option().map(from_struct).unwrap_or_default();
        Self::parse(&item.r#type, &common, &data)
    }

    fn parse(kind: &str, common: &Map<String, Value>, data: &Map<String, Value>) -> Self {
        let id = string(common, "id");
        let response_id = opt_string(common, "response_id").filter(|r| !r.is_empty());
        let status = string(common, "status");
        match kind {
            "message" => {
                let content = data
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                Self::Message {
                    id,
                    response_id,
                    role: Role::from_wire(&opt_string(data, "role").unwrap_or_default()),
                    text: joined_text(&content, ""),
                    content,
                }
            }
            "function_call" => Self::FunctionCall {
                id,
                response_id,
                call_id: string(data, "call_id"),
                name: string(data, "name"),
                arguments: match data.get("arguments") {
                    Some(Value::String(arguments)) => arguments.clone(),
                    Some(Value::Null) | None => String::new(),
                    Some(other) => other.to_string(),
                },
                status,
            },
            "function_call_output" => Self::FunctionCallOutput {
                id,
                response_id,
                call_id: string(data, "call_id"),
                output: match data.get("output") {
                    Some(Value::String(output)) => output.clone(),
                    Some(Value::Null) | None => String::new(),
                    Some(other) => other.to_string(),
                },
                status,
            },
            "reasoning" => {
                let block_text = |key: &str| {
                    data.get(key)
                        .and_then(Value::as_array)
                        .map(|blocks| joined_text(blocks, "\n\n"))
                        .unwrap_or_default()
                };
                let text = block_text("content");
                Self::Reasoning {
                    id,
                    response_id,
                    text: if text.is_empty() {
                        block_text("summary")
                    } else {
                        text
                    },
                }
            }
            "error" => Self::Error {
                id,
                response_id,
                source: string(data, "source"),
                message: string(data, "message"),
            },
            _ => {
                let mut item = common.clone();
                item.extend(data.iter().map(|(k, v)| (k.clone(), v.clone())));
                Self::Other {
                    kind: kind.to_owned(),
                    item,
                }
            }
        }
    }
}

/// An event of a session's stream.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// A `response.*` lifecycle event.
    Response {
        /// Which one.
        status: ResponseStatus,
        /// The response (turn) id.
        response_id: Option<String>,
        /// What failed (`llm`, `execution`, `tool`, `harness`), on `response.failed`.
        source: Option<String>,
        /// The error message of a failed or incomplete response.
        error: Option<String>,
    },
    /// `response.output_text.delta`: streamed text of the agent's message.
    OutputTextDelta {
        /// The text.
        delta: String,
    },
    /// `response.reasoning_text.delta` or `response.reasoning_summary_text.delta`.
    ReasoningDelta {
        /// The text.
        delta: String,
    },
    /// `response.output_item.done`.
    OutputItemDone(OutputItem),
    /// `response.error`: a retryable or reported error during the turn.
    Error {
        /// `llm`, `execution`, `tool` or `harness`.
        source: String,
        /// The tool, when `source` is `tool`.
        tool_name: Option<String>,
        /// The error code.
        code: String,
        /// The message.
        message: String,
    },
    /// `response.elicitation_request`.
    ElicitationRequest(ElicitationRequest),
    /// `session.input.consumed`: a posted input entered the history (a user message
    /// another client sent, or one queued behind a turn).
    InputConsumed {
        /// The history item's id.
        item_id: String,
        /// Its item, read like a history item.
        item: OutputItem,
    },
    /// `session.status`: `idle`, `launching`, `running`, `waiting` or `failed`.
    SessionStatus {
        /// The status.
        status: String,
    },
    /// Any other event, as sent.
    Unknown {
        /// The event type.
        kind: String,
        /// Its payload (a string when it isn't JSON).
        data: Value,
    },
}

impl Event {
    /// Read a frame: its `event:` name (or the payload's `type`) and JSON payload.
    pub fn parse(event: Option<&str>, data: &str) -> Self {
        let value: Value = match serde_json::from_str(data) {
            Ok(value) => value,
            Err(_) => {
                return Self::Unknown {
                    kind: event.unwrap_or_default().to_owned(),
                    data: Value::String(data.to_owned()),
                };
            }
        };
        let empty = Map::new();
        let object = value.as_object().unwrap_or(&empty);
        let kind = event
            .filter(|e| !e.is_empty())
            .map(str::to_owned)
            .or_else(|| opt_string(object, "type"))
            .unwrap_or_default();
        let kind = normalize_kind(&kind);
        Self::from_json(&kind, object).unwrap_or(Self::Unknown { kind, data: value })
    }

    fn from_json(kind: &str, data: &Map<String, Value>) -> Option<Self> {
        if let Some(status) = ResponseStatus::from_wire(kind) {
            let response = data.get("response").and_then(Value::as_object);
            let response_id = response
                .and_then(|r| opt_string(r, "id"))
                .or_else(|| opt_string(data, "response_id"));
            let error = response
                .and_then(|r| r.get("error"))
                .and_then(Value::as_object)
                .and_then(|e| opt_string(e, "message"))
                .or_else(|| {
                    response
                        .and_then(|r| r.get("incomplete_details"))
                        .and_then(Value::as_object)
                        .and_then(|d| opt_string(d, "reason"))
                });
            return Some(Self::Response {
                status,
                response_id,
                source: opt_string(data, "source"),
                error,
            });
        }
        Some(match kind {
            "response.output_text.delta" => Self::OutputTextDelta {
                delta: string(data, "delta"),
            },
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                Self::ReasoningDelta {
                    delta: string(data, "delta"),
                }
            }
            "response.output_item.done" => {
                Self::OutputItemDone(OutputItem::from_stream(data.get("item")?.as_object()?))
            }
            "response.error" => {
                let error = data.get("error").and_then(Value::as_object);
                Self::Error {
                    source: string(data, "source"),
                    tool_name: opt_string(data, "tool_name"),
                    code: error.map(|e| string(e, "code")).unwrap_or_default(),
                    message: error.map(|e| string(e, "message")).unwrap_or_default(),
                }
            }
            "response.elicitation_request" => {
                Self::ElicitationRequest(ElicitationRequest::from_json(data))
            }
            "session.input.consumed" => {
                let payload = data.get("data")?.as_object()?;
                let item_id = string(payload, "item_id");
                let mut common = Map::new();
                common.insert("id".into(), Value::String(item_id.clone()));
                common.insert("status".into(), Value::String("completed".into()));
                let inner = payload
                    .get("data")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                Self::InputConsumed {
                    item: OutputItem::parse(&string(payload, "type"), &common, &inner),
                    item_id,
                }
            }
            "session.status" => Self::SessionStatus {
                status: string(data, "status"),
            },
            _ => return None,
        })
    }
}

/// The server renders some terminal events from a Python enum
/// (`response.TaskStatus.COMPLETED`); read them as `response.completed`, as the SDK does.
fn normalize_kind(kind: &str) -> String {
    match kind.split_once(".TaskStatus.") {
        Some((prefix, status)) => format!("{prefix}.{}", status.to_lowercase()),
        None => kind.to_owned(),
    }
}

/// The text of content blocks: each `{text}` block's, joined with `separator`.
fn joined_text(blocks: &[Value], separator: &str) -> String {
    blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(separator)
}

fn opt_string(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn string(map: &Map<String, Value>, key: &str) -> String {
    opt_string(map, key).unwrap_or_default()
}
