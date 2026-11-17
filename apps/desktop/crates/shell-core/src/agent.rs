//! Agent sessions on an instance's Omnigent: whether the agent can run, a session's
//! commands, and its stream folded into chat messages for the UI.
//!
//! The UI reaches Omnigent only through the shell's commands (no HTTP client of its own);
//! the shell calls it with [`omnigent_client`] at the instance's gateway root of the
//! `omnigent` module. Locally Omnigent runs without auth (`OMNIGENT_AUTH_ENABLED=0`); a
//! remote upstream's bearer token, if it has one, goes along.
//!
//! The messages are shaped like chatcn's `ChatcnMessage`, which mirrors the AI SDK's
//! `UIMessage`: `{id, role, parts}`, with `text`, `reasoning` and `dynamic-tool` parts
//! (Omnigent's tools are only known at run time, so every call is a dynamic tool).
//!
//! The stream has no replay. [`Sessions::subscribe`] opens it, then reads the snapshot, so
//! nothing falls between them; [`Sessions::send`] waits for the stream to show the turn
//! (or [`SEND_WAIT`]). When the stream drops, the subscription reconnects the same way and
//! sends the snapshot again.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use futures::future::{AbortHandle, Abortable};
use omnigent_client::models::omnigent::v1::ListAgentsRequest;
use omnigent_client::{
    Conversation, ElicitationRequest, HttpClient, OmnigentClient, Part, Role, ToolCall,
    ToolState as FoldState,
};
use serde::Serialize;
use tokio::sync::Notify;

use crate::Json;
use crate::query::bearer_of;
use crate::services::Upstream;

/// The module Omnigent runs as, and the key the UI addresses it by.
pub const OMNIGENT: &str = "omnigent";
/// The agent the desktop's chat runs (`agents/lakehouse/`, `just agent-install`).
pub const AGENT_NAME: &str = "lakehouse";
/// Where a session's runner starts on the host: the omnigent-host container's working
/// directory in trestle's `omnigent` module.
pub const DEFAULT_WORKSPACE: &str = "/workspace";
/// How long [`Sessions::send`] waits for the stream to show the turn.
pub const SEND_WAIT: Duration = Duration::from_millis(1_500);
/// The first wait before a dropped stream reconnects; it doubles up to [`MAX_BACKOFF`].
const BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Why an agent command failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentError {
    /// The instance doesn't run Omnigent.
    #[error("the environment doesn't run Omnigent")]
    NoOmnigent,
    /// The agent isn't installed on the instance's Omnigent.
    #[error("the agent `{name}` isn't installed (just agent-install)")]
    NoAgent { name: String },
    /// No host is online to run the agent.
    #[error("no Omnigent host is online")]
    NoHost,
    /// Omnigent answered with an error.
    #[error("Omnigent: {message}")]
    Api { status: u16, message: String },
    /// Omnigent couldn't be reached, or answered something unreadable.
    #[error("{message}")]
    Failed { message: String },
}

impl From<omnigent_client::Error> for AgentError {
    fn from(e: omnigent_client::Error) -> Self {
        match e {
            omnigent_client::Error::Api(api) => AgentError::Api {
                status: api.status,
                message: api.message,
            },
            other => AgentError::Failed {
                message: other.to_string(),
            },
        }
    }
}

/// Whether the agent can run on an instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    /// Omnigent answered.
    pub reachable: bool,
    /// The agent [`AGENT_NAME`], if it is installed.
    pub agent_id: Option<String>,
    /// A host is online to run it.
    pub host_online: bool,
    /// Why Omnigent couldn't be asked, when it wasn't reachable.
    pub error: Option<String>,
}

/// A session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub agent_id: String,
    /// `idle`, `running`, `waiting` or `failed`.
    pub status: String,
    pub title: Option<String>,
}

/// What became of a message sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SendAck {
    /// Queued for the agent (else persisted straight into the history).
    pub queued: bool,
    /// The session's stream showed an event within [`SEND_WAIT`]: the turn is under way.
    /// `false` without a subscription.
    pub observed: bool,
}

/// Who wrote a message (chatcn's `role`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

/// Whether a text part is complete (the AI SDK's text `state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum TextState {
    Streaming,
    Done,
}

/// A tool call's state (chatcn's `ToolState`, the AI SDK's).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum ToolState {
    InputStreaming,
    InputAvailable,
    ApprovalRequested,
    ApprovalResponded,
    OutputAvailable,
    OutputError,
    OutputDenied,
}

/// The approval a policy asked for a tool call (the AI SDK's `approval`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ToolApproval {
    /// The elicitation to resolve.
    pub id: String,
    /// The answer, once given.
    pub approved: Option<bool>,
    pub reason: Option<String>,
}

/// A part of a message (chatcn's `ChatcnPart`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type")]
pub enum ChatPart {
    #[serde(rename = "text")]
    Text { text: String, state: TextState },
    #[serde(rename = "reasoning")]
    Reasoning { text: String, state: TextState },
    /// A tool call. `toolName` is Omnigent's: `<server>__<tool>` for an MCP server's tool
    /// (`uc__open_grant_form`).
    #[serde(rename = "dynamic-tool", rename_all = "camelCase")]
    DynamicTool {
        tool_call_id: String,
        tool_name: String,
        state: ToolState,
        /// The arguments (`null` when they aren't JSON).
        input: Json,
        /// The result, as the agent saw it (Omnigent flattens MCP results to text).
        output: Option<String>,
        error_text: Option<String>,
        approval: Option<ToolApproval>,
    },
}

/// A chat message (chatcn's `ChatcnMessage`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub parts: Vec<ChatPart>,
}

/// An open elicitation: a policy's approval, or an MCP server's form.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Elicitation {
    pub elicitation_id: String,
    /// `form` or `url`.
    pub mode: String,
    pub message: String,
    /// The form's JSON Schema.
    pub requested_schema: Option<Json>,
    /// The policy phase (`pre_tool_use`); `None` for an MCP server's form.
    pub phase: Option<String>,
    pub policy_name: Option<String>,
    pub content_preview: Option<String>,
}

/// A message on a session's channel.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionMessage {
    /// The whole conversation: first, and again after each reconnect. Replaces what the
    /// UI holds.
    Snapshot {
        status: Option<String>,
        messages: Vec<ChatMessage>,
        elicitations: Vec<Elicitation>,
    },
    /// A message is new or changed: replace the one with its id, or append it.
    Upsert { message: ChatMessage },
    /// The open elicitations changed.
    Elicitations { elicitations: Vec<Elicitation> },
    /// The session status changed (`idle`, `launching`, `running`, `waiting`, `failed`).
    Status { status: String },
    /// The stream dropped; it reconnects after a wait, then sends a snapshot.
    Reconnecting { attempt: u32, error: Option<String> },
}

/// The app's agent sessions: one HTTP client for every Omnigent, and the subscriptions.
#[derive(Default)]
pub struct Agents {
    http: reqwest::Client,
    sessions: Sessions,
}

impl Agents {
    /// A client of the Omnigent at `upstream`.
    pub fn client(&self, upstream: &Upstream) -> OmnigentClient {
        client(&self.http, upstream)
    }

    /// The subscriptions.
    pub fn sessions(&self) -> &Sessions {
        &self.sessions
    }
}

/// A client of the Omnigent at `upstream` (the gateway root of its module), over `http`.
pub fn client(http: &reqwest::Client, upstream: &Upstream) -> OmnigentClient {
    let mut transport = HttpClient::new(http.clone());
    if let Some(token) = bearer_of(upstream) {
        transport = transport.with_token(token);
    }
    OmnigentClient::with_transport(transport, upstream.base_url.clone())
}

/// Whether [`AGENT_NAME`] can run: installed, with a host online. An Omnigent that
/// doesn't answer is reported, not an error.
pub async fn status(client: &OmnigentClient) -> AgentStatus {
    let list_hosts = Default::default();
    let agent = find_agent(client);
    let hosts = client.hosts().list_hosts(&list_hosts);
    match futures::try_join!(agent, hosts) {
        Ok((agent, hosts)) => AgentStatus {
            reachable: true,
            agent_id: agent,
            host_online: hosts.hosts.iter().any(|h| h.status == "online"),
            error: None,
        },
        Err(e) => AgentStatus {
            reachable: false,
            agent_id: None,
            host_online: false,
            error: Some(e.to_string()),
        },
    }
}

/// The id of [`AGENT_NAME`]: the caller's own agent (`omnigent agent add`, what `just
/// agent-install` makes), else a server agent of that name. The server's list
/// (`GET /v1/agents`) never has the caller's agents; `scope=user` lists them, newest
/// first.
async fn find_agent(client: &OmnigentClient) -> omnigent_client::Result<Option<String>> {
    let mut request = ListAgentsRequest {
        scope: Some("user".into()),
        ..Default::default()
    };
    loop {
        let page = client.agents().list_agents(&request).await?;
        if let Some(agent) = page.data.iter().find(|a| a.name == AGENT_NAME) {
            return Ok(Some(agent.id.clone()));
        }
        match page.last_id {
            Some(last) if page.has_more && request.after.as_ref() != Some(&last) => {
                request.after = Some(last);
            }
            _ => break,
        }
    }
    let server = client.agents().list_agents(&Default::default()).await?;
    Ok(server
        .data
        .iter()
        .find(|a| a.name == AGENT_NAME)
        .map(|a| a.id.clone()))
}

/// A new session of [`AGENT_NAME`], on the first online host (in its
/// [`DEFAULT_WORKSPACE`]).
pub async fn create(
    client: &OmnigentClient,
    title: Option<String>,
) -> Result<SessionInfo, AgentError> {
    let agent_id = find_agent(client)
        .await?
        .ok_or_else(|| AgentError::NoAgent {
            name: AGENT_NAME.into(),
        })?;
    let hosts = client.hosts().list_hosts(&Default::default()).await?;
    let host = hosts
        .hosts
        .iter()
        .find(|h| h.status == "online" && h.sandbox_provider.is_none())
        .ok_or(AgentError::NoHost)?;
    let session = client
        .create_session(&agent_id, |r| {
            r.title = title;
            r.host_id = Some(host.host_id.clone());
            r.workspace = Some(DEFAULT_WORKSPACE.into());
        })
        .await?;
    Ok(SessionInfo {
        id: session.id,
        agent_id: session.agent_id,
        status: session.status,
        title: session.title,
    })
}

/// The open subscriptions, one per session.
#[derive(Default)]
pub struct Sessions {
    subscriptions: Arc<Mutex<HashMap<String, Subscription>>>,
    generation: AtomicU64,
    backoff: Option<Duration>,
}

struct Subscription {
    generation: u64,
    abort: AbortHandle,
    /// Woken by each event of the stream.
    events: Arc<Notify>,
}

impl Sessions {
    /// With `backoff` as the first wait before a reconnect (for tests).
    pub fn with_backoff(backoff: Duration) -> Self {
        Sessions {
            backoff: Some(backoff),
            ..Default::default()
        }
    }

    /// Subscribe to `session_id`: open its stream and read its snapshot, then return the
    /// pump, which the caller spawns. The pump sends `sink` the snapshot, then the changes,
    /// reconnecting when the stream drops, until `sink` returns `false` (the UI is gone),
    /// [`Sessions::unsubscribe`], or another subscription to the session replaces it.
    pub async fn subscribe<S>(
        &self,
        client: OmnigentClient,
        session_id: &str,
        mut sink: S,
    ) -> Result<impl Future<Output = ()> + Send + 'static, AgentError>
    where
        S: FnMut(SessionMessage) -> bool + Send + 'static,
    {
        let (stream, conversation) = open(&client, session_id).await?;
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let events = Arc::new(Notify::new());
        let (abort, registration) = AbortHandle::new_pair();
        let replaced = self
            .subscriptions
            .lock()
            .expect("subscriptions lock")
            .insert(
                session_id.to_owned(),
                Subscription {
                    generation,
                    abort,
                    events: events.clone(),
                },
            );
        if let Some(old) = replaced {
            old.abort.abort();
        }
        let subscriptions = self.subscriptions.clone();
        let session_id = session_id.to_owned();
        let backoff = self.backoff.unwrap_or(BACKOFF);
        Ok(async move {
            let pump = pump(
                client,
                session_id.clone(),
                stream,
                conversation,
                events,
                backoff,
                &mut sink,
            );
            let _ = Abortable::new(pump, registration).await;
            let mut subscriptions = subscriptions.lock().expect("subscriptions lock");
            if subscriptions
                .get(&session_id)
                .is_some_and(|s| s.generation == generation)
            {
                subscriptions.remove(&session_id);
            }
        })
    }

    /// Stop the subscription to `session_id`, if there is one.
    pub fn unsubscribe(&self, session_id: &str) {
        let removed = self
            .subscriptions
            .lock()
            .expect("subscriptions lock")
            .remove(session_id);
        if let Some(subscription) = removed {
            subscription.abort.abort();
        }
    }

    /// Whether `session_id` has a subscription.
    pub fn is_subscribed(&self, session_id: &str) -> bool {
        self.subscriptions
            .lock()
            .expect("subscriptions lock")
            .contains_key(session_id)
    }

    /// Send the user's `text`. With a subscription, waits for its stream's next event
    /// (the turn starting, or the message queued), at most [`SEND_WAIT`].
    pub async fn send(
        &self,
        client: &OmnigentClient,
        session_id: &str,
        text: &str,
    ) -> Result<SendAck, AgentError> {
        let events = self
            .subscriptions
            .lock()
            .expect("subscriptions lock")
            .get(session_id)
            .map(|s| s.events.clone());
        let Some(events) = events else {
            let ack = client.send_message(session_id, text).await?;
            return Ok(SendAck {
                queued: ack.queued,
                observed: false,
            });
        };
        // Armed before the post: an event that comes while it is in flight counts.
        let mut next = std::pin::pin!(events.notified());
        next.as_mut().enable();
        let ack = client.send_message(session_id, text).await?;
        let observed = tokio::time::timeout(SEND_WAIT, next).await.is_ok();
        Ok(SendAck {
            queued: ack.queued,
            observed,
        })
    }
}

/// Interrupt `session_id`'s turn.
pub async fn interrupt(client: &OmnigentClient, session_id: &str) -> Result<(), AgentError> {
    client.interrupt(session_id).await?;
    Ok(())
}

/// The stream, then the snapshot: an event between the two is in both, and the fold
/// takes it once.
async fn open(
    client: &OmnigentClient,
    session_id: &str,
) -> Result<(omnigent_client::EventStream, Conversation), AgentError> {
    let stream = client.stream(session_id).await?;
    let snapshot = client.snapshot(session_id).await?;
    Ok((stream, Conversation::from_session(&snapshot)))
}

async fn pump<S>(
    client: OmnigentClient,
    session_id: String,
    mut stream: omnigent_client::EventStream,
    mut conversation: Conversation,
    events: Arc<Notify>,
    backoff: Duration,
    sink: &mut S,
) where
    S: FnMut(SessionMessage) -> bool,
{
    loop {
        if !sink(snapshot(&conversation)) {
            return;
        }
        let mut error = None;
        while let Some(event) = stream.next().await {
            let event = match event {
                Ok(event) => event,
                Err(e) => {
                    error = Some(e.to_string());
                    break;
                }
            };
            events.notify_waiters();
            for message in changes(&mut conversation, &event) {
                if !sink(message) {
                    return;
                }
            }
        }
        // Dropped (or ended): reconnect until it opens again.
        let mut attempt = 0u32;
        let mut wait = backoff;
        loop {
            attempt += 1;
            if !sink(SessionMessage::Reconnecting {
                attempt,
                error: error.take(),
            }) {
                return;
            }
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(MAX_BACKOFF);
            match open(&client, &session_id).await {
                Ok((next, folded)) => {
                    stream = next;
                    conversation = folded;
                    break;
                }
                Err(e) => error = Some(e.to_string()),
            }
        }
    }
}

/// Fold `event` into `conversation`, and say what changed.
fn changes(conversation: &mut Conversation, event: &omnigent_client::Event) -> Vec<SessionMessage> {
    let status = conversation.status().map(str::to_owned);
    let elicitations: Vec<String> = elicitation_ids(conversation);
    let changed = conversation.apply(event);
    let mut out: Vec<SessionMessage> = changed
        .iter()
        .filter_map(|id| conversation.message(id))
        .map(|m| SessionMessage::Upsert {
            message: chat_message(m),
        })
        .collect();
    if elicitation_ids(conversation) != elicitations {
        out.push(SessionMessage::Elicitations {
            elicitations: conversation
                .elicitations()
                .iter()
                .map(elicitation)
                .collect(),
        });
    }
    if let Some(now) = conversation.status()
        && status.as_deref() != Some(now)
    {
        out.push(SessionMessage::Status { status: now.into() });
    }
    out
}

fn elicitation_ids(conversation: &Conversation) -> Vec<String> {
    conversation
        .elicitations()
        .iter()
        .map(|e| e.elicitation_id.clone())
        .collect()
}

fn snapshot(conversation: &Conversation) -> SessionMessage {
    SessionMessage::Snapshot {
        status: conversation.status().map(str::to_owned),
        messages: conversation.messages().iter().map(chat_message).collect(),
        elicitations: conversation
            .elicitations()
            .iter()
            .map(elicitation)
            .collect(),
    }
}

/// A folded message as chatcn's.
pub fn chat_message(message: &omnigent_client::Message) -> ChatMessage {
    ChatMessage {
        id: message.id.clone(),
        role: match message.role {
            Role::User => ChatRole::User,
            Role::Assistant => ChatRole::Assistant,
            Role::System => ChatRole::System,
        },
        parts: message.parts.iter().map(chat_part).collect(),
    }
}

fn text_state(streaming: bool) -> TextState {
    if streaming {
        TextState::Streaming
    } else {
        TextState::Done
    }
}

fn chat_part(part: &Part) -> ChatPart {
    match part {
        Part::Text { text, streaming } => ChatPart::Text {
            text: text.clone(),
            state: text_state(*streaming),
        },
        Part::Reasoning { text, streaming } => ChatPart::Reasoning {
            text: text.clone(),
            state: text_state(*streaming),
        },
        Part::Tool(call) => tool_part(call),
    }
}

fn tool_part(call: &ToolCall) -> ChatPart {
    ChatPart::DynamicTool {
        tool_call_id: call.call_id.clone(),
        tool_name: call.name.clone(),
        state: match call.state {
            FoldState::InputStreaming => ToolState::InputStreaming,
            FoldState::InputAvailable => ToolState::InputAvailable,
            FoldState::ApprovalRequested => ToolState::ApprovalRequested,
            FoldState::ApprovalResponded => ToolState::ApprovalResponded,
            FoldState::OutputAvailable => ToolState::OutputAvailable,
            FoldState::OutputError => ToolState::OutputError,
            FoldState::OutputDenied => ToolState::OutputDenied,
        },
        input: Json(call.input.clone()),
        output: call.output.clone(),
        error_text: call.error.clone(),
        approval: call.approval.as_ref().map(|a| ToolApproval {
            id: a.id.clone(),
            approved: a.approved,
            reason: a.reason.clone(),
        }),
    }
}

fn elicitation(e: &ElicitationRequest) -> Elicitation {
    Elicitation {
        elicitation_id: e.elicitation_id.clone(),
        mode: e.mode.clone(),
        message: e.message.clone(),
        requested_schema: e.requested_schema.clone().map(Json),
        phase: e.phase.clone(),
        policy_name: e.policy_name.clone(),
        content_preview: e.content_preview.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    /// A fake Omnigent: answers by method and path, any number of connections. The
    /// stream route sends its body and closes, so a subscriber reconnects.
    async fn omnigent(
        routes: Vec<(&'static str, &'static str, u16, String)>,
    ) -> (Upstream, mpsc::UnboundedReceiver<(String, String, Value)>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/omnigent/", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::unbounded_channel();
        let routes = Arc::new(routes);
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let routes = routes.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 4096];
                    let (head, mut body) = loop {
                        let n = socket.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            break (
                                String::from_utf8_lossy(&raw[..end]).into_owned(),
                                raw[end + 4..].to_vec(),
                            );
                        }
                    };
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    while body.len() < length {
                        let n = socket.read(&mut buf).await.unwrap();
                        body.extend_from_slice(&buf[..n]);
                    }
                    let mut start = head.lines().next().unwrap().split(' ');
                    let method = start.next().unwrap().to_owned();
                    let target = start.next().unwrap().to_owned();
                    let path = target.split('?').next().unwrap().to_owned();
                    let _ = tx.send((
                        method.clone(),
                        target.clone(),
                        serde_json::from_slice(&body).unwrap_or(Value::Null),
                    ));
                    let (status, answer) = routes
                        .iter()
                        // A route with a query matches only that query.
                        .find(|(m, p, _, _)| *m == method && (*p == path || *p == target))
                        .map(|(_, p, status, body)| {
                            let kind = if p.ends_with("/stream") {
                                "text/event-stream"
                            } else {
                                "application/json"
                            };
                            (*status, (kind, body.clone()))
                        })
                        .unwrap_or((404, ("application/json", "{}".into())));
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        answer.0,
                        answer.1.len(),
                        answer.1
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        let upstream = Upstream::new(url.parse().unwrap(), crate::services::Placement::Local);
        (upstream, rx)
    }

    fn agents() -> String {
        json!({"object": "list", "has_more": false, "data": [
            {"id": "agent_other", "name": "other", "created_at": 1},
            {"id": "agent_lh", "name": "lakehouse", "created_at": 1}]})
        .to_string()
    }

    fn hosts(status: &str) -> String {
        json!({"hosts": [
            {"host_id": "sandbox", "name": "modal", "status": "online", "sandbox_provider": "modal"},
            // As v0.21 and v0.22 report it: readiness by harness.
            {"host_id": "host_1", "name": "omnigent-host", "status": status,
             "configured_harnesses": {"claude-sdk": true, "pi-native": "needs-auth"}}]})
        .to_string()
    }

    fn session() -> String {
        json!({"id": "conv_1", "agent_id": "agent_lh", "status": "idle", "created_at": 1,
               "title": "Grant", "items": []})
        .to_string()
    }

    fn http() -> reqwest::Client {
        reqwest::Client::new()
    }

    #[tokio::test]
    async fn status_of_the_agent() {
        let (up, _) = omnigent(vec![
            ("GET", "/omnigent/v1/agents", 200, agents()),
            ("GET", "/omnigent/v1/hosts", 200, hosts("online")),
        ])
        .await;
        let status = status(&client(&http(), &up)).await;
        assert_eq!(
            status,
            AgentStatus {
                reachable: true,
                agent_id: Some("agent_lh".into()),
                host_online: true,
                error: None
            }
        );

        let (up, _) = omnigent(vec![]).await;
        let down = super::status(&client(&http(), &up)).await;
        assert!(!down.reachable);
        assert!(down.error.is_some());
    }

    #[tokio::test]
    async fn finds_the_agent_among_the_callers_own_then_the_servers() {
        let none = json!({"object": "list", "has_more": false, "data": []}).to_string();
        // `just agent-install` makes a user agent, which only `scope=user` lists.
        let (up, mut seen) = omnigent(vec![
            ("GET", "/omnigent/v1/agents?scope=user", 200, agents()),
            ("GET", "/omnigent/v1/agents", 200, none.clone()),
            ("GET", "/omnigent/v1/hosts", 200, hosts("online")),
        ])
        .await;
        let found = status(&client(&http(), &up)).await;
        assert_eq!(found.agent_id.as_deref(), Some("agent_lh"));
        let mut targets = Vec::new();
        while let Ok((_, target, _)) = seen.try_recv() {
            targets.push(target);
        }
        assert!(
            targets
                .iter()
                .any(|t| t == "/omnigent/v1/agents?scope=user"),
            "{targets:?}"
        );
        // A server agent of that name does too.
        let (up, _) = omnigent(vec![
            ("GET", "/omnigent/v1/agents?scope=user", 200, none),
            ("GET", "/omnigent/v1/agents", 200, agents()),
            ("GET", "/omnigent/v1/hosts", 200, hosts("online")),
        ])
        .await;
        let found = status(&client(&http(), &up)).await;
        assert_eq!(found.agent_id.as_deref(), Some("agent_lh"));
    }

    #[tokio::test]
    async fn creates_a_session_on_the_online_host() {
        let (up, mut seen) = omnigent(vec![
            ("GET", "/omnigent/v1/agents", 200, agents()),
            ("GET", "/omnigent/v1/hosts", 200, hosts("online")),
            ("POST", "/omnigent/v1/sessions", 201, session()),
        ])
        .await;
        let info = create(&client(&http(), &up), Some("Grant".into()))
            .await
            .unwrap();
        assert_eq!(info.id, "conv_1");
        let mut requests = Vec::new();
        while let Ok(r) = seen.try_recv() {
            requests.push(r);
        }
        let (_, _, body) = requests
            .iter()
            .find(|(m, _, _)| m == "POST")
            .expect("the create");
        assert_eq!(
            body,
            &json!({"agent_id": "agent_lh", "title": "Grant", "host_id": "host_1", "workspace": "/workspace"})
        );

        let (up, _) = omnigent(vec![
            ("GET", "/omnigent/v1/agents", 200, agents()),
            ("GET", "/omnigent/v1/hosts", 200, hosts("offline")),
        ])
        .await;
        assert_eq!(
            create(&client(&http(), &up), None).await.unwrap_err(),
            AgentError::NoHost
        );
    }

    const TURN: &str = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"status\":\"completed\",\"response_id\":\"resp_1\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hi!\"}]}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"status\":\"completed\",\"response_id\":\"resp_1\",\"name\":\"uc__open_grant_form\",\"arguments\":\"{\\\"full_name\\\":\\\"main.default.t\\\"}\",\"call_id\":\"call_1\"}}\n\n",
        "event: session.status\n",
        "data: {\"type\":\"session.status\",\"conversation_id\":\"conv_1\",\"status\":\"running\"}\n\n",
    );

    fn resynced() -> String {
        json!({"id": "conv_1", "agent_id": "agent_lh", "status": "running", "created_at": 1,
               "items": [
                   {"id": "msg_1", "type": "message", "status": "completed", "response_id": "resp_1",
                    "created_at": 2, "data": {"role": "assistant", "content": [{"type": "output_text", "text": "Hi!"}]}},
                   {"id": "fc_1", "type": "function_call", "status": "completed", "response_id": "resp_1",
                    "created_at": 3, "data": {"name": "uc__open_grant_form", "arguments": "{}", "call_id": "call_1"}}
               ],
               "pending_elicitations": [{"elicitation_id": "e1", "params": {"message": "Run it?", "phase": "pre_tool_use"}}]})
        .to_string()
    }

    #[tokio::test]
    async fn subscribes_folds_and_resyncs_on_reconnect() {
        let (up, _) = omnigent(vec![
            (
                "GET",
                "/omnigent/v1/sessions/conv_1/stream",
                200,
                TURN.into(),
            ),
            ("GET", "/omnigent/v1/sessions/conv_1", 200, resynced()),
        ])
        .await;
        let sessions = Sessions::with_backoff(Duration::from_millis(10));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let snapshots = Arc::new(AtomicU64::new(0));
        let counted = snapshots.clone();
        let pump = sessions
            .subscribe(client(&http(), &up), "conv_1", move |m| {
                let snapshot = matches!(m, SessionMessage::Snapshot { .. });
                let _ = tx.send(m);
                // Stop after the snapshot of the first reconnect.
                !(snapshot && counted.fetch_add(1, Ordering::Relaxed) == 1)
            })
            .await
            .unwrap();
        assert!(sessions.is_subscribed("conv_1"));
        pump.await;
        assert!(
            !sessions.is_subscribed("conv_1"),
            "a finished pump unregisters"
        );

        let mut messages = Vec::new();
        while let Ok(m) = rx.try_recv() {
            messages.push(m);
        }
        // The first snapshot holds what the server had (here, already the turn).
        let SessionMessage::Snapshot {
            messages: first,
            elicitations,
            ..
        } = &messages[0]
        else {
            panic!("a snapshot first: {messages:?}");
        };
        assert_eq!(first.len(), 1);
        assert_eq!(elicitations.len(), 1);
        // The stream repeats the turn: nothing changes but the status...
        let upserts = messages
            .iter()
            .filter(|m| matches!(m, SessionMessage::Upsert { .. }))
            .count();
        assert!(upserts <= 2, "{messages:#?}");
        assert!(
            messages.contains(&SessionMessage::Status {
                status: "running".into()
            }) || matches!(&messages[0], SessionMessage::Snapshot { status: Some(s), .. } if s == "running")
        );
        // ...then it drops, reconnects, and resyncs.
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, SessionMessage::Reconnecting { attempt: 1, .. }))
        );
        let SessionMessage::Snapshot { messages: last, .. } = messages.last().unwrap() else {
            panic!("a snapshot last");
        };
        assert_eq!(last, first);
        let ChatPart::DynamicTool {
            tool_name,
            state,
            approval,
            ..
        } = &last[0].parts[1]
        else {
            panic!("the tool part: {last:?}");
        };
        assert_eq!(tool_name, "uc__open_grant_form");
        assert_eq!(*state, ToolState::ApprovalRequested);
        assert_eq!(approval.as_ref().unwrap().id, "e1");
    }

    #[tokio::test]
    async fn folds_a_live_turn_into_upserts() {
        let (up, _) = omnigent(vec![
            (
                "GET",
                "/omnigent/v1/sessions/conv_1/stream",
                200,
                TURN.into(),
            ),
            ("GET", "/omnigent/v1/sessions/conv_1", 200, session()),
        ])
        .await;
        let sessions = Sessions::with_backoff(Duration::from_millis(10));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let pump = sessions
            .subscribe(client(&http(), &up), "conv_1", move |m| {
                let reconnecting = matches!(m, SessionMessage::Reconnecting { .. });
                let _ = tx.send(m);
                !reconnecting
            })
            .await
            .unwrap();
        pump.await;
        let mut messages = Vec::new();
        while let Ok(m) = rx.try_recv() {
            messages.push(m);
        }
        let upserts: Vec<&ChatMessage> = messages
            .iter()
            .filter_map(|m| match m {
                SessionMessage::Upsert { message } => Some(message),
                _ => None,
            })
            .collect();
        // created, the delta, the final text, the tool call.
        assert_eq!(upserts.len(), 4, "{messages:#?}");
        assert_eq!(
            upserts[1].parts,
            [ChatPart::Text {
                text: "Hi".into(),
                state: TextState::Streaming
            }]
        );
        let last = upserts.last().unwrap();
        assert_eq!(last.id, "resp_1");
        assert_eq!(last.role, ChatRole::Assistant);
        assert_eq!(
            serde_json::to_value(last).unwrap(),
            json!({"id": "resp_1", "role": "assistant", "parts": [
                {"type": "text", "text": "Hi!", "state": "done"},
                {"type": "dynamic-tool", "toolCallId": "call_1", "toolName": "uc__open_grant_form",
                 "state": "input-available", "input": {"full_name": "main.default.t"},
                 "output": null, "errorText": null, "approval": null}
            ]})
        );
        assert!(messages.contains(&SessionMessage::Status {
            status: "running".into()
        }));
    }

    #[tokio::test]
    async fn send_waits_for_the_stream() {
        let ack = json!({"queued": true}).to_string();
        let (up, mut seen) = omnigent(vec![(
            "POST",
            "/omnigent/v1/sessions/conv_1/events",
            202,
            ack,
        )])
        .await;
        let sessions = Sessions::default();
        // No subscription: no wait.
        let sent = sessions
            .send(&client(&http(), &up), "conv_1", "hello")
            .await
            .unwrap();
        assert_eq!(
            sent,
            SendAck {
                queued: true,
                observed: false
            }
        );
        let (_, target, body) = seen.recv().await.unwrap();
        assert_eq!(target, "/omnigent/v1/sessions/conv_1/events");
        assert_eq!(body["type"], "message");
        assert_eq!(body["data"]["content"][0]["text"], "hello");

        // A subscription that sees an event: observed.
        let events = Arc::new(Notify::new());
        let (abort, _registration) = AbortHandle::new_pair();
        sessions.subscriptions.lock().unwrap().insert(
            "conv_1".into(),
            Subscription {
                generation: 0,
                abort,
                events: events.clone(),
            },
        );
        let notifier = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            events.notify_waiters();
        });
        let sent = sessions
            .send(&client(&http(), &up), "conv_1", "again")
            .await
            .unwrap();
        notifier.await.unwrap();
        assert!(sent.observed);
        sessions.unsubscribe("conv_1");
        assert!(!sessions.is_subscribed("conv_1"));
    }

    #[test]
    fn errors_serialize_tagged() {
        assert_eq!(
            serde_json::to_value(AgentError::NoAgent {
                name: "lakehouse".into()
            })
            .unwrap(),
            json!({"kind": "noAgent", "name": "lakehouse"})
        );
    }
}
