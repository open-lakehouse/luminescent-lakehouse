//! The fold of a session's events, or of its snapshot, into chat messages.
//!
//! A port of the Python SDK's `BlockStream` (`omnigent_client/_stream.py`): its blocks are
//! for a terminal, these are messages with parts, the shape of the AI SDK's `UIMessage`
//! (which chatcn's `ChatcnMessage` mirrors). The rules it keeps:
//!
//! - one tool call per `call_id`, and one result per `call_id`, however often the server
//!   repeats them;
//! - a `message` item with no streamed text before it adds its text; one after streamed
//!   text replaces that text with the final;
//! - each `response.created` starts the agent's message of that turn.
//!
//! The fold is idempotent over the same items, so a client that reconnects can rebuild it
//! from a snapshot ([`Conversation::from_session`]) and apply the stream's events on top:
//! the ones the snapshot already has change nothing.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::api::Session;
use crate::events::{ElicitationRequest, Event, OutputItem, ResponseStatus, Role};
use crate::from_struct;

/// The state of a tool call, as the AI SDK names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolState {
    /// The arguments are still streaming (Omnigent sends a call whole, so a fold never
    /// produces it; it is here for the full set).
    InputStreaming,
    /// The call is made and runs.
    InputAvailable,
    /// A policy asks the user to approve the call.
    ApprovalRequested,
    /// The user answered; the call runs, or is denied, next.
    ApprovalResponded,
    /// The result is in.
    OutputAvailable,
    /// The call failed.
    OutputError,
    /// The user denied the call.
    OutputDenied,
}

impl ToolState {
    /// The AI SDK's name: `input-available`, `output-error`, ...
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InputStreaming => "input-streaming",
            Self::InputAvailable => "input-available",
            Self::ApprovalRequested => "approval-requested",
            Self::ApprovalResponded => "approval-responded",
            Self::OutputAvailable => "output-available",
            Self::OutputError => "output-error",
            Self::OutputDenied => "output-denied",
        }
    }

    /// Whether the call has no result yet.
    pub fn is_pending(self) -> bool {
        matches!(
            self,
            Self::InputStreaming
                | Self::InputAvailable
                | Self::ApprovalRequested
                | Self::ApprovalResponded
        )
    }
}

/// An approval asked for a tool call: the elicitation and, once answered, the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    /// The elicitation id.
    pub id: String,
    /// The answer, once given.
    pub approved: Option<bool>,
    /// Why, if the user said.
    pub reason: Option<String>,
}

/// A tool name, split: `uc__open_grant_form` is the `open_grant_form` tool of the MCP
/// server `uc`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolName<'a> {
    /// The MCP server, as the agent's config names it; `None` for a built-in tool.
    pub server: Option<&'a str>,
    /// The tool.
    pub tool: &'a str,
}

impl<'a> ToolName<'a> {
    /// Split `name`. Omnigent names an MCP server's tool `<server>__<tool>`
    /// (`omnigent/runner/mcp_manager.py`); the Claude Code harness sees Omnigent's tools
    /// behind its in-process MCP server, as `mcp__omnigent__<server>__<tool>`, and that
    /// prefix is dropped too.
    pub fn parse(name: &'a str) -> Self {
        let name = name.strip_prefix("mcp__omnigent__").unwrap_or(name);
        match name.split_once("__") {
            Some((server, tool)) if !server.is_empty() && !tool.is_empty() => Self {
                server: Some(server),
                tool,
            },
            _ => Self {
                server: None,
                tool: name,
            },
        }
    }
}

/// A tool call and what became of it.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    /// The call id.
    pub call_id: String,
    /// The tool, as Omnigent names it ([`ToolName::parse`] splits it).
    pub name: String,
    /// The arguments, parsed (`Value::Null` when they are not JSON; see `arguments`).
    pub input: Value,
    /// The arguments as sent.
    pub arguments: String,
    /// Its state.
    pub state: ToolState,
    /// The result, with [`ToolState::OutputAvailable`].
    pub output: Option<String>,
    /// The error, with [`ToolState::OutputError`].
    pub error: Option<String>,
    /// The approval asked, if a policy asked one.
    pub approval: Option<Approval>,
}

/// A part of a message.
// A chat holds few parts: a plain `ToolCall` keeps them simple to build and match.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    /// Text; `streaming` while deltas still arrive.
    Text {
        /// The text so far.
        text: String,
        /// Whether more may come.
        streaming: bool,
    },
    /// The agent's reasoning.
    Reasoning {
        /// The text so far.
        text: String,
        /// Whether more may come.
        streaming: bool,
    },
    /// A tool call.
    Tool(ToolCall),
}

/// A chat message.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    /// The id: the history item's for a user message, the response's for the agent's.
    pub id: String,
    /// The author.
    pub role: Role,
    /// The parts, in order.
    pub parts: Vec<Part>,
}

/// A session's conversation, folded from its events.
#[derive(Clone, Debug, Default)]
pub struct Conversation {
    messages: Vec<Message>,
    /// The agent's message of the current turn.
    current: Option<usize>,
    /// call id → (message, part) of the tool call.
    calls: HashMap<String, (usize, usize)>,
    /// The history items already folded.
    seen: HashSet<String>,
    /// The elicitations not answered yet.
    elicitations: Vec<ElicitationRequest>,
    /// The session status, last reported.
    status: Option<String>,
    /// Turns started (`response.created`) for which no message exists yet.
    synthetic: usize,
}

impl Conversation {
    /// The conversation of a snapshot (`GET /v1/sessions/{id}?include_items=true`): its
    /// history and its open elicitations.
    pub fn from_session(session: &Session) -> Self {
        let mut conversation = Self::default();
        for item in &session.items {
            let item = OutputItem::from_conversation_item(item);
            conversation.apply_item(item);
        }
        conversation.finish_streaming();
        conversation.current = None;
        for elicitation in &session.pending_elicitations {
            let elicitation = ElicitationRequest::from_json(&from_struct(elicitation));
            conversation.elicitation(elicitation);
        }
        conversation.status = Some(session.status.clone());
        conversation
    }

    /// The messages, in order.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// The message with id `id`.
    pub fn message(&self, id: &str) -> Option<&Message> {
        self.messages.iter().find(|m| m.id == id)
    }

    /// The elicitations not answered yet, oldest first.
    pub fn elicitations(&self) -> &[ElicitationRequest] {
        &self.elicitations
    }

    /// The session status, as last reported (`idle`, `running`, ...).
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Fold `event` in. Returns the ids of the messages it changed (none for an event that
    /// changes no message, such as a status).
    pub fn apply(&mut self, event: &Event) -> Vec<String> {
        let before = self.messages.clone();
        match event {
            Event::Response {
                status,
                response_id,
                ..
            } => match status {
                ResponseStatus::Created | ResponseStatus::Queued | ResponseStatus::InProgress => {
                    self.start_turn(response_id.as_deref());
                }
                ResponseStatus::Completed
                | ResponseStatus::Failed
                | ResponseStatus::Cancelled
                | ResponseStatus::Incomplete => {
                    if let Some(index) = self.current.take() {
                        finish_message(&mut self.messages[index]);
                    }
                }
            },
            Event::OutputTextDelta { delta } => {
                let index = self.current_message();
                push_delta(&mut self.messages[index], delta, false);
            }
            Event::ReasoningDelta { delta } => {
                let index = self.current_message();
                push_delta(&mut self.messages[index], delta, true);
            }
            Event::OutputItemDone(item) => self.apply_item(item.clone()),
            Event::InputConsumed { item, .. } => self.apply_item(item.clone()),
            Event::Error {
                source,
                tool_name: Some(tool_name),
                message,
                ..
            } if source == "tool" => {
                // The latest pending call of that tool failed.
                if let Some(call) = self
                    .pending_calls_mut()
                    .rev()
                    .find(|c| &c.name == tool_name)
                {
                    call.state = ToolState::OutputError;
                    call.error = Some(message.clone());
                }
            }
            Event::ElicitationRequest(elicitation) => self.elicitation(elicitation.clone()),
            Event::SessionStatus { status } => self.status = Some(status.clone()),
            _ => {}
        }
        changed(&before, &self.messages)
    }

    /// Record the user's answer to the elicitation `elicitation_id`, once sent: its tool
    /// call becomes [`ToolState::ApprovalResponded`] and the elicitation is no longer open.
    /// Returns the ids of the messages it changed.
    pub fn resolve_elicitation(&mut self, elicitation_id: &str, approved: bool) -> Vec<String> {
        let before = self.messages.clone();
        self.elicitations
            .retain(|e| e.elicitation_id != elicitation_id);
        for call in self.calls_mut() {
            if let Some(approval) = call.approval.as_mut().filter(|a| a.id == elicitation_id) {
                approval.approved = Some(approved);
                if call.state == ToolState::ApprovalRequested {
                    call.state = ToolState::ApprovalResponded;
                }
            }
        }
        changed(&before, &self.messages)
    }

    fn apply_item(&mut self, item: OutputItem) {
        if let Some(id) = item.id().filter(|id| !id.is_empty())
            && !self.seen.insert(id.to_owned())
        {
            // Already folded: but deltas that a snapshot already holds as this item may
            // have been replayed on top of it; drop them.
            let (response_id, reasoning) = match &item {
                OutputItem::Message {
                    role: Role::Assistant,
                    response_id,
                    ..
                } => (response_id, false),
                OutputItem::Reasoning { response_id, .. } => (response_id, true),
                _ => return,
            };
            if let Some(index) = self.turn_message(response_id.as_deref()) {
                let parts = &mut self.messages[index].parts;
                let replayed = match parts.last() {
                    Some(Part::Text { streaming, .. }) => *streaming && !reasoning,
                    Some(Part::Reasoning { streaming, .. }) => *streaming && reasoning,
                    _ => false,
                };
                if replayed {
                    parts.pop();
                }
            }
            return;
        }
        match item {
            OutputItem::Message {
                id,
                role: Role::Assistant,
                response_id,
                text,
                ..
            } => {
                let index = self.turn_message_or_start(response_id.as_deref(), &id);
                let parts = &mut self.messages[index].parts;
                match parts.last_mut() {
                    Some(Part::Text {
                        text: streamed,
                        streaming: streaming @ true,
                    }) => {
                        if !text.is_empty() {
                            *streamed = text;
                        }
                        *streaming = false;
                    }
                    _ if !text.is_empty() => parts.push(Part::Text {
                        text,
                        streaming: false,
                    }),
                    _ => {}
                }
            }
            OutputItem::Message { id, role, text, .. } => {
                // The current turn goes on: a message posted while it runs is queued.
                self.messages.push(Message {
                    id,
                    role,
                    parts: vec![Part::Text {
                        text,
                        streaming: false,
                    }],
                });
            }
            OutputItem::FunctionCall {
                id,
                response_id,
                call_id,
                name,
                arguments,
                status,
            } => {
                let state = match status.as_str() {
                    "failed" | "incomplete" => ToolState::OutputError,
                    _ => ToolState::InputAvailable,
                };
                if let Some(call) = self.call_mut(&call_id) {
                    // A repeat refreshes the call (the SDK's `pending_tools`).
                    call.arguments = arguments.clone();
                    call.input = parse_input(&arguments);
                    if state == ToolState::OutputError && call.state.is_pending() {
                        call.state = state;
                    }
                    return;
                }
                let index = self.turn_message_or_start(response_id.as_deref(), &id);
                let message = &mut self.messages[index];
                finish_streaming_parts(message);
                message.parts.push(Part::Tool(ToolCall {
                    call_id: call_id.clone(),
                    name,
                    input: parse_input(&arguments),
                    arguments,
                    state,
                    output: None,
                    error: None,
                    approval: None,
                }));
                self.calls.insert(call_id, (index, message.parts.len() - 1));
            }
            OutputItem::FunctionCallOutput {
                id,
                response_id,
                call_id,
                output,
                status,
            } => {
                if self.call_mut(&call_id).is_none() {
                    // A result whose call isn't folded (a history cut short): show it alone.
                    self.apply_item(OutputItem::FunctionCall {
                        id: format!("{id}:call"),
                        response_id,
                        call_id: call_id.clone(),
                        name: String::new(),
                        arguments: String::new(),
                        status: String::new(),
                    });
                }
                let Some(call) = self.call_mut(&call_id) else {
                    return;
                };
                let denied = call
                    .approval
                    .as_ref()
                    .is_some_and(|a| a.approved == Some(false));
                if denied {
                    call.state = ToolState::OutputDenied;
                    call.error = Some(output);
                } else if status == "failed" {
                    call.state = ToolState::OutputError;
                    call.error = Some(output);
                } else {
                    call.state = ToolState::OutputAvailable;
                    call.output = Some(output);
                }
                let approval = call.approval.as_ref().map(|a| a.id.clone());
                if let Some(approval) = approval {
                    self.elicitations.retain(|e| e.elicitation_id != approval);
                }
            }
            OutputItem::Reasoning {
                id,
                response_id,
                text,
            } => {
                let index = self.turn_message_or_start(response_id.as_deref(), &id);
                let parts = &mut self.messages[index].parts;
                match parts.last_mut() {
                    Some(Part::Reasoning {
                        text: streamed,
                        streaming: streaming @ true,
                    }) => {
                        if !text.is_empty() {
                            *streamed = text;
                        }
                        *streaming = false;
                    }
                    _ if !text.is_empty() => parts.push(Part::Reasoning {
                        text,
                        streaming: false,
                    }),
                    _ => {}
                }
            }
            OutputItem::Error {
                source, message, ..
            } if source == "tool" => {
                if let Some(call) = self.pending_calls_mut().next_back() {
                    call.state = ToolState::OutputError;
                    call.error = Some(message);
                }
            }
            OutputItem::Error { .. } | OutputItem::Other { .. } => {}
        }
    }

    fn elicitation(&mut self, elicitation: ElicitationRequest) {
        if self
            .elicitations
            .iter()
            .any(|e| e.elicitation_id == elicitation.elicitation_id)
        {
            return;
        }
        // An approval has no call id: it is the latest call still waiting for one.
        let id = elicitation.elicitation_id.clone();
        if elicitation.phase.is_some()
            && let Some(call) = self
                .pending_calls_mut()
                .rev()
                .find(|c| c.state == ToolState::InputAvailable && c.approval.is_none())
        {
            call.state = ToolState::ApprovalRequested;
            call.approval = Some(Approval {
                id,
                approved: None,
                reason: None,
            });
        }
        self.elicitations.push(elicitation);
    }

    fn start_turn(&mut self, response_id: Option<&str>) {
        if let Some(index) = self.turn_message(response_id) {
            self.current = Some(index);
            return;
        }
        if let Some(index) = self.current.take() {
            finish_message(&mut self.messages[index]);
        }
        let id = match response_id {
            Some(id) => id.to_owned(),
            None => self.synthetic_id(),
        };
        self.messages.push(Message {
            id,
            role: Role::Assistant,
            parts: Vec::new(),
        });
        self.current = Some(self.messages.len() - 1);
    }

    /// The agent's message of the turn `response_id`.
    fn turn_message(&self, response_id: Option<&str>) -> Option<usize> {
        let response_id = response_id?;
        self.messages
            .iter()
            .rposition(|m| m.role == Role::Assistant && m.id == response_id)
    }

    /// The agent's message for an item of the turn `response_id`: that turn's, the current
    /// one, or a new one.
    fn turn_message_or_start(&mut self, response_id: Option<&str>, item_id: &str) -> usize {
        if let Some(index) = self.turn_message(response_id) {
            return index;
        }
        if response_id.is_none()
            && let Some(index) = self.current
        {
            return index;
        }
        let id = response_id.unwrap_or(item_id).to_owned();
        self.start_turn(Some(&id));
        self.current.unwrap_or(self.messages.len() - 1)
    }

    fn current_message(&mut self) -> usize {
        match self.current {
            Some(index) => index,
            None => {
                self.start_turn(None);
                self.messages.len() - 1
            }
        }
    }

    fn synthetic_id(&mut self) -> String {
        self.synthetic += 1;
        format!("turn_{}", self.synthetic)
    }

    fn call_mut(&mut self, call_id: &str) -> Option<&mut ToolCall> {
        let &(message, part) = self.calls.get(call_id)?;
        match self.messages.get_mut(message)?.parts.get_mut(part)? {
            Part::Tool(call) => Some(call),
            _ => None,
        }
    }

    fn calls_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut ToolCall> {
        self.messages
            .iter_mut()
            .flat_map(|m| m.parts.iter_mut())
            .filter_map(|p| match p {
                Part::Tool(call) => Some(call),
                _ => None,
            })
    }

    fn pending_calls_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut ToolCall> {
        self.calls_mut().filter(|c| c.state.is_pending())
    }

    fn finish_streaming(&mut self) {
        for message in &mut self.messages {
            finish_streaming_parts(message);
        }
    }
}

fn push_delta(message: &mut Message, delta: &str, reasoning: bool) {
    match (message.parts.last_mut(), reasoning) {
        (
            Some(Part::Text {
                text,
                streaming: true,
            }),
            false,
        )
        | (
            Some(Part::Reasoning {
                text,
                streaming: true,
            }),
            true,
        ) => text.push_str(delta),
        _ => {
            finish_streaming_parts(message);
            let text = delta.to_owned();
            message.parts.push(match reasoning {
                false => Part::Text {
                    text,
                    streaming: true,
                },
                true => Part::Reasoning {
                    text,
                    streaming: true,
                },
            });
        }
    }
}

fn finish_streaming_parts(message: &mut Message) {
    for part in &mut message.parts {
        if let Part::Text { streaming, .. } | Part::Reasoning { streaming, .. } = part {
            *streaming = false;
        }
    }
}

fn finish_message(message: &mut Message) {
    finish_streaming_parts(message);
}

fn parse_input(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return Value::Object(Default::default());
    }
    serde_json::from_str(arguments).unwrap_or(Value::Null)
}

fn changed(before: &[Message], after: &[Message]) -> Vec<String> {
    after
        .iter()
        .enumerate()
        .filter(|(i, m)| before.get(*i) != Some(*m))
        .map(|(_, m)| m.id.clone())
        .collect()
}
