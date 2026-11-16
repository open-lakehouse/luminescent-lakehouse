//! Agent sessions on an instance's Omnigent (shell-core's `agent`): whether the agent can
//! run, a session's commands, and its stream on a channel, folded into chat messages.
//!
//! Each command names its instance; its client is built for that instance's `omnigent`
//! module (the gateway root, with the app's credential for it when remote).

use shell_core::agent::{
    self, AgentError, AgentStatus, OMNIGENT, SendAck, SessionInfo, SessionMessage,
};
use shell_core::services::Upstream;
use tauri::State;
use tauri::ipc::Channel;

use crate::AppState;

/// Where `instance`'s Omnigent is.
async fn omnigent(state: &AppState, instance: &str) -> Result<Upstream, AgentError> {
    let failed = |message: String| AgentError::Failed { message };
    let runner = state.runner.clone().map_err(|e| failed(e.to_string()))?;
    let local = runner
        .bindings(instance)
        .map_err(|e| failed(e.to_string()))?;
    let remote = runner
        .function_roots(instance)
        .await
        .map_err(|e| failed(e.to_string()))?;
    state
        .directory
        .resolve_instance(&local, &remote, OMNIGENT)
        .map_err(failed)?
        .ok_or(AgentError::NoOmnigent)
}

/// Whether the `lakehouse` agent can run on `instance`: Omnigent answers, the agent is
/// installed, a host is online.
#[tauri::command]
#[specta::specta]
pub async fn agent_status(
    state: State<'_, AppState>,
    instance: String,
) -> Result<AgentStatus, AgentError> {
    let client = state.agent.client(&omnigent(&state, &instance).await?);
    Ok(agent::status(&client).await)
}

/// A new session of the `lakehouse` agent on `instance`, on its online host.
#[tauri::command]
#[specta::specta]
pub async fn session_create(
    state: State<'_, AppState>,
    instance: String,
    title: Option<String>,
) -> Result<SessionInfo, AgentError> {
    let client = state.agent.client(&omnigent(&state, &instance).await?);
    agent::create(&client, title).await
}

/// Send the user's `text` to the session. Subscribe first ([`session_subscribe`]): the
/// stream has no replay. Waits for the stream to show the turn, 1.5 s at most.
#[tauri::command]
#[specta::specta]
pub async fn session_send(
    state: State<'_, AppState>,
    instance: String,
    session_id: String,
    text: String,
) -> Result<SendAck, AgentError> {
    let client = state.agent.client(&omnigent(&state, &instance).await?);
    state
        .agent
        .sessions()
        .send(&client, &session_id, &text)
        .await
}

/// Interrupt the session's running turn.
#[tauri::command]
#[specta::specta]
pub async fn session_interrupt(
    state: State<'_, AppState>,
    instance: String,
    session_id: String,
) -> Result<(), AgentError> {
    let client = state.agent.client(&omnigent(&state, &instance).await?);
    agent::interrupt(&client, &session_id).await
}

/// Follow the session on `on_event`: a snapshot, then the messages that change, the open
/// elicitations and the status; after a dropped stream, `reconnecting` and a new snapshot.
/// Resolves once the stream is open, so a message sent afterwards is seen. Replaces an
/// earlier subscription to the session; ends with [`session_unsubscribe`], or when the
/// webview goes.
#[tauri::command]
#[specta::specta]
pub async fn session_subscribe(
    state: State<'_, AppState>,
    instance: String,
    session_id: String,
    on_event: Channel<SessionMessage>,
) -> Result<(), AgentError> {
    let client = state.agent.client(&omnigent(&state, &instance).await?);
    let pump = state
        .agent
        .sessions()
        .subscribe(client, &session_id, move |message| {
            on_event.send(message).is_ok()
        })
        .await?;
    tauri::async_runtime::spawn(pump);
    Ok(())
}

/// Stop following the session.
#[tauri::command]
#[specta::specta]
pub fn session_unsubscribe(state: State<'_, AppState>, session_id: String) {
    state.agent.sessions().unsubscribe(&session_id);
}
