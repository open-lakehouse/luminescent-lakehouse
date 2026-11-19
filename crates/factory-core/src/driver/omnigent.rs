//! [`OmnigentDriver`] over `omnigent-client` (Omnigent 0.22).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::BoxStream;
use omnigent_client::{
    ElicitationAction, ElicitationRequest, Event, OmnigentClient, ResponseStatus, api,
};
use serde_json::{Map, Value};

use super::{
    AgentRef, Ask, AskAction, Created, DetectedPr, DetectedPrs, DriverEvent, HarnessState,
    HostInfo, OmnigentDriver, SessionRequest, SessionSnapshot, SessionStatus, Target,
};
use crate::{Error, Result};

/// The largest page `GET /v1/agents` serves.
const AGENTS_LIMIT: i32 = 100;

/// [`OmnigentDriver`] over an [`OmnigentClient`].
pub struct OmnigentAdapter {
    client: OmnigentClient,
    /// Agent name → its resolution, refreshed on a miss.
    agents: Mutex<HashMap<String, AgentRef>>,
}

impl OmnigentAdapter {
    pub fn new(client: OmnigentClient) -> Self {
        Self {
            client,
            agents: Mutex::new(HashMap::new()),
        }
    }

    pub fn client(&self) -> &OmnigentClient {
        &self.client
    }

    async fn list_agents(&self, scope: Option<&str>) -> Result<Vec<api::Agent>> {
        let mut out = Vec::new();
        let mut after = None;
        loop {
            let page = self
                .client
                .agents()
                .list_agents(&api::ListAgentsRequest {
                    limit: Some(AGENTS_LIMIT),
                    after: after.take(),
                    scope: scope.map(str::to_owned),
                    ..Default::default()
                })
                .await
                .map_err(map_err)?;
            let last = page.data.last().map(|a| a.id.clone());
            out.extend(page.data);
            match (page.has_more, page.last_id.or(last)) {
                (true, Some(last)) => after = Some(last),
                _ => return Ok(out),
            }
        }
    }
}

fn map_err(e: omnigent_client::Error) -> Error {
    if e.is_not_found() {
        Error::NotFound(e.to_string())
    } else {
        Error::internal(format!("omnigent: {e}"))
    }
}

fn agent_ref(a: &api::Agent) -> AgentRef {
    AgentRef {
        id: a.id.clone(),
        name: a.name.clone(),
        version: a.version.to_string(),
        harness: a.harness.clone(),
    }
}

fn harness_state(value: &Value) -> HarnessState {
    match value {
        Value::Bool(true) => HarnessState::Ready,
        Value::Bool(false) | Value::Null => HarnessState::Disabled,
        Value::String(why) => HarnessState::Unavailable(why.clone()),
        other => HarnessState::Unavailable(other.to_string()),
    }
}

/// A host as placement sees it.
pub fn host_info(host: &api::Host) -> HostInfo {
    let harnesses = host
        .configured_harnesses
        .as_option()
        .and_then(|s| serde_json::to_value(s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
        .iter()
        .map(|(k, v)| (k.clone(), harness_state(v)))
        .collect();
    HostInfo {
        id: host.host_id.clone(),
        name: host.name.clone(),
        online: host.status == "online",
        sandbox_provider: host.sandbox_provider.clone(),
        harnesses,
    }
}

fn ask(e: &ElicitationRequest) -> Ask {
    Ask {
        elicitation_id: e.elicitation_id.clone(),
        message: e.message.clone(),
        tool: e.policy_name.clone().unwrap_or_default(),
    }
}

/// A session as the factory sees it.
pub fn snapshot(s: &api::Session) -> SessionSnapshot {
    let asks = s
        .pending_elicitations
        .iter()
        .filter_map(|p| serde_json::to_value(p).ok())
        .filter_map(|v| v.as_object().map(ElicitationRequest::from_json))
        .map(|e| ask(&e))
        .collect();
    SessionSnapshot {
        id: s.id.clone(),
        status: SessionStatus::parse(&s.status),
        labels: s
            .labels
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        cost_usd: s.total_cost_usd.unwrap_or_default(),
        last_error: s
            .last_task_error
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        harness: s.harness.clone(),
        branch: s.git_branch.clone(),
        asks,
        turns: turns(s),
    }
}

/// The turns a session has had: the distinct response ids (`resp_…`) of its
/// items, and the running one. A user message's item carries its input's id
/// (`turn_…`) and a resource event the session's, so neither counts.
fn turns(s: &api::Session) -> u32 {
    let mut ids: std::collections::HashSet<&str> = s
        .items
        .iter()
        .map(|i| i.response_id.as_str())
        .filter(|id| id.starts_with("resp_"))
        .collect();
    if let Some(active) = &s.active_response_id {
        ids.insert(active);
    }
    u32::try_from(ids.len()).unwrap_or(u32::MAX)
}

fn detected(pr: &api::PullRequest) -> DetectedPr {
    DetectedPr {
        url: pr.url.clone(),
        repo: pr.repository.clone(),
        number: u32::try_from(pr.number).unwrap_or_default(),
        title: pr.title.clone().unwrap_or_default(),
        state: pr.state.clone().unwrap_or_default().to_lowercase(),
        draft: pr.is_draft,
        relationship: pr.relationship.clone(),
    }
}

/// What a stream event means to the factory; `None` for the rest (text
/// deltas, tool items, heartbeats).
pub fn driver_event(event: &Event) -> Option<DriverEvent> {
    match event {
        Event::Response {
            status: ResponseStatus::InProgress,
            ..
        } => Some(DriverEvent::TurnStarted),
        Event::Response {
            status: ResponseStatus::Failed,
            error,
            source,
            ..
        } => Some(DriverEvent::TurnFailed(match (source, error) {
            (Some(s), Some(e)) => format!("{s}: {e}"),
            (_, Some(e)) => e.clone(),
            (Some(s), None) => s.clone(),
            (None, None) => "the turn failed".into(),
        })),
        Event::SessionStatus { status } => Some(DriverEvent::Status(SessionStatus::parse(status))),
        Event::ElicitationRequest(e) => Some(DriverEvent::Ask(ask(e))),
        _ => None,
    }
}

fn initial_message(text: &str) -> Result<api::SessionEventInput> {
    let data = serde_json::json!({
        "role": "user",
        "content": [{"type": "input_text", "text": text}],
    });
    let Value::Object(map) = data else {
        unreachable!()
    };
    Ok(api::SessionEventInput {
        r#type: "message".into(),
        data: omnigent_client::to_struct(map).map_err(map_err)?.into(),
        ..Default::default()
    })
}

#[async_trait]
impl OmnigentDriver for OmnigentAdapter {
    async fn resolve_agent(&self, name: &str) -> Result<AgentRef> {
        if let Some(hit) = self.agents.lock().expect("agents lock").get(name) {
            return Ok(hit.clone());
        }
        for scope in [Some("user"), None] {
            let agents = self.list_agents(scope).await?;
            // The newest version, if a name is listed more than once.
            if let Some(found) = agents
                .iter()
                .filter(|a| a.name == name)
                .max_by_key(|a| a.version)
            {
                let found = agent_ref(found);
                self.agents
                    .lock()
                    .expect("agents lock")
                    .insert(name.into(), found.clone());
                return Ok(found);
            }
        }
        Err(Error::NotFound(format!("agent {name}")))
    }

    async fn hosts(&self) -> Result<Vec<HostInfo>> {
        let list = self
            .client
            .hosts()
            .list_hosts(&api::ListHostsRequest::default())
            .await
            .map_err(map_err)?;
        Ok(list.hosts.iter().map(host_info).collect())
    }

    async fn create_or_find(&self, request: &SessionRequest) -> Result<Created> {
        let found = self
            .client
            .find_sessions(&request.labels.identity(), true)
            .await
            .map_err(map_err)?;
        if let Some(existing) = found.first() {
            return Ok(Created {
                session: self.snapshot(&existing.id).await?,
                created: false,
            });
        }
        let first = initial_message(&request.prompt)?;
        let session = self
            .client
            .create_session(&request.agent_id, |r| {
                r.title = Some(request.title.clone());
                r.labels = request.labels.all().into_iter().collect();
                match &request.target {
                    Target::Host { host_id, workspace } => {
                        r.host_type = Some("external".into());
                        r.host_id = Some(host_id.clone());
                        r.workspace = Some(workspace.clone());
                    }
                    Target::Managed {
                        provider,
                        sandbox_profile,
                        workspace,
                    } => {
                        r.host_type = Some("managed".into());
                        r.sandbox_provider = Some(provider.clone());
                        r.sandbox_profile_id =
                            (!sandbox_profile.is_empty()).then(|| sandbox_profile.clone());
                        r.workspace = Some(workspace.clone());
                    }
                }
                if let Some(branch) = &request.branch {
                    r.git = api::GitOptions {
                        branch_name: branch.clone(),
                        base_branch: request.base_branch.clone(),
                        ..Default::default()
                    }
                    .into();
                }
                r.harness_override = request.harness.clone();
                r.model_override = request.model.clone();
                r.reasoning_effort = request.reasoning_effort.clone();
                r.initial_items = vec![first];
            })
            .await
            .map_err(map_err)?;
        Ok(Created {
            session: snapshot(&session),
            created: true,
        })
    }

    async fn snapshot(&self, session: &str) -> Result<SessionSnapshot> {
        let s = self.client.snapshot(session).await.map_err(map_err)?;
        Ok(snapshot(&s))
    }

    async fn pull_requests(&self, session: &str) -> Result<DetectedPrs> {
        let (prs, synced) = self.client.pull_requests(session).await.map_err(map_err)?;
        Ok(DetectedPrs {
            prs: prs.iter().map(detected).collect(),
            synced,
        })
    }

    async fn send(&self, session: &str, text: &str) -> Result<()> {
        self.client
            .send_message(session, text)
            .await
            .map(drop)
            .map_err(map_err)
    }

    async fn interrupt(&self, session: &str) -> Result<()> {
        self.client
            .interrupt(session)
            .await
            .map(drop)
            .map_err(map_err)
    }

    async fn resolve_ask(
        &self,
        session: &str,
        elicitation_id: &str,
        action: AskAction,
        content: Option<Map<String, Value>>,
    ) -> Result<()> {
        let action = match action {
            AskAction::Accept => ElicitationAction::Accept,
            AskAction::Decline => ElicitationAction::Decline,
            AskAction::Cancel => ElicitationAction::Cancel,
        };
        self.client
            .resolve_elicitation(session, elicitation_id, action, content)
            .await
            .map(drop)
            .map_err(map_err)
    }

    async fn events(&self, session: &str) -> Result<BoxStream<'static, DriverEvent>> {
        let stream = self.client.stream(session).await.map_err(map_err)?;
        Ok(follow(stream, self.client.clone(), session.to_owned()))
    }
}

/// The factory's events from a session's stream: the stream's own, plus the
/// session's PRs whenever it goes idle, and [`DriverEvent::Disconnected`]
/// when the stream ends or fails.
pub fn follow<S>(
    stream: S,
    client: OmnigentClient,
    session: String,
) -> BoxStream<'static, DriverEvent>
where
    S: futures::Stream<Item = omnigent_client::Result<Event>> + Send + Unpin + 'static,
{
    struct St<S> {
        stream: Option<S>,
        ready: VecDeque<DriverEvent>,
        client: OmnigentClient,
        session: String,
    }
    let state = St {
        stream: Some(stream),
        ready: VecDeque::new(),
        client,
        session,
    };
    futures::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(next) = st.ready.pop_front() {
                return Some((next, st));
            }
            let stream = st.stream.as_mut()?;
            match stream.next().await {
                Some(Ok(event)) => {
                    let Some(e) = driver_event(&event) else {
                        continue;
                    };
                    let idle = e == DriverEvent::Status(SessionStatus::Idle);
                    st.ready.push_back(e);
                    if idle {
                        // PRs aren't on the stream; read them when a turn ends.
                        match st.client.pull_requests(&st.session).await {
                            Ok((prs, synced)) => {
                                st.ready.push_back(DriverEvent::PullRequests(DetectedPrs {
                                    prs: prs.iter().map(detected).collect(),
                                    synced,
                                }))
                            }
                            Err(e) => tracing::warn!(session = %st.session, "pull requests: {e}"),
                        }
                    }
                }
                Some(Err(e)) => {
                    tracing::debug!(session = %st.session, "stream: {e}");
                    st.stream = None;
                    return Some((DriverEvent::Disconnected, st));
                }
                None => {
                    st.stream = None;
                    return Some((DriverEvent::Disconnected, st));
                }
            }
        }
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use omnigent_client::EventStream;

    const GRANT_TURN: &str = include_str!("../../../omnigent-client/fixtures/grant-turn.sse");
    const GRANT_SESSION: &str =
        include_str!("../../../omnigent-client/fixtures/grant-turn.session.json");

    fn events_of(sse: &str) -> Vec<DriverEvent> {
        let body = futures::stream::iter([Ok::<_, reqwest::Error>(Bytes::from(sse.to_owned()))]);
        let mut stream = EventStream::from_body(body);
        let mut out = Vec::new();
        futures::executor::block_on(async {
            while let Some(Ok(e)) = stream.next().await {
                out.extend(driver_event(&e));
            }
        });
        out
    }

    #[test]
    fn the_recorded_turns_start_and_go_idle() {
        let events = events_of(GRANT_TURN);
        let idle = DriverEvent::Status(SessionStatus::Idle);
        let started = events
            .iter()
            .filter(|e| **e == DriverEvent::TurnStarted)
            .count();
        assert_eq!(started, 2, "{events:?}");
        // Every turn starts after the session runs and ends idle.
        let first_start = events
            .iter()
            .position(|e| *e == DriverEvent::TurnStarted)
            .unwrap();
        let first_idle = events.iter().position(|e| *e == idle).unwrap();
        assert!(first_start < first_idle);
        assert_eq!(events.last(), Some(&idle));
    }

    #[test]
    fn the_recorded_snapshot_maps() {
        let session: api::Session = serde_json::from_str(GRANT_SESSION).unwrap();
        let snap = snapshot(&session);
        assert_eq!(snap.id, session.id);
        assert_eq!(snap.status, SessionStatus::parse(&session.status));
        assert_eq!(snap.turns, 2, "the recording has two turns");
        assert!(snap.last_error.is_empty());
    }

    #[test]
    fn hosts_read_their_harnesses() {
        let host: api::Host = serde_json::from_value(serde_json::json!({
            "host_id": "h1",
            "name": "local",
            "owner": "me",
            "status": "online",
            "configured_harnesses": {"claude-sdk": true, "codex": "needs-auth", "pi": false},
        }))
        .unwrap();
        let info = host_info(&host);
        assert!(info.online);
        assert_eq!(info.harnesses["claude-sdk"], HarnessState::Ready);
        assert_eq!(
            info.harnesses["codex"],
            HarnessState::Unavailable("needs-auth".into())
        );
        assert_eq!(info.harnesses["pi"], HarnessState::Disabled);
    }

    #[test]
    fn failed_turns_say_why() {
        let e = Event::Response {
            status: ResponseStatus::Failed,
            response_id: None,
            source: Some("llm".into()),
            error: Some("overloaded".into()),
        };
        assert_eq!(
            driver_event(&e),
            Some(DriverEvent::TurnFailed("llm: overloaded".into()))
        );
    }
}
