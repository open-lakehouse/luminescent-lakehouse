//! [`OmnigentDriver`] over `omnigent-client` (Omnigent 0.22).
//!
//! The adapter builds its client per call, so each carries the token its
//! [`TokenSource`] has now (a service principal's, renewed before it
//! expires) and, for a session on a connected host, that host's slice key
//! ([`SLICE_KEY_HEADER`]).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::BoxStream;
pub use omnigent_client::PermissionLevel;
use omnigent_client::{
    ElicitationAction, ElicitationRequest, Event, HttpClient, OmnigentClient, ResponseStatus, api,
};
use serde_json::{Map, Value};
use url::Url;

use super::{
    AgentRef, Ask, AskAction, Created, DetectedPr, DetectedPrs, DriverEvent, HarnessState,
    HostInfo, OmnigentDriver, SLICE_KEY_HEADER, SessionRequest, SessionSnapshot, SessionStatus,
    SliceKeys, Target,
};
use crate::token::{StaticToken, TokenSource};
use crate::{Error, Result};

/// The largest page `GET /v1/agents` serves.
const AGENTS_LIMIT: i32 = 100;

/// A principal every dispatched session is shared with, and at what level: `principal=level`
/// (`data-eng=read`, `alice@example.com=manage`).
///
/// The factory owns the sessions it starts, so on a server with accounts nobody else sees
/// them until they are shared. The principal is a user or a group; a group's members
/// see the session only if the server's permission store resolves group membership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionShare {
    pub principal: String,
    pub level: PermissionLevel,
}

impl std::str::FromStr for SessionShare {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let (principal, level) = s
            .rsplit_once('=')
            .ok_or_else(|| format!("a share is principal=level, not {s:?}"))?;
        let principal = principal.trim();
        if principal.is_empty() {
            return Err(format!("a share names a principal: {s:?}"));
        }
        Ok(Self {
            principal: principal.to_owned(),
            level: level.parse()?,
        })
    }
}

/// A comma-separated list of [`SessionShare`]s, each principal at most once:
/// `data-eng=read,alice@example.com=manage`. Empty entries are skipped.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionShares(pub Vec<SessionShare>);

impl std::str::FromStr for SessionShares {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut shares: Vec<SessionShare> = Vec::new();
        for entry in s.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let share: SessionShare = entry.parse()?;
            if shares.iter().any(|have| have.principal == share.principal) {
                return Err(format!("{} is shared twice", share.principal));
            }
            shares.push(share);
        }
        Ok(Self(shares))
    }
}

/// [`OmnigentDriver`] over an [`OmnigentClient`] of the server at a base
/// URL, built per call with the current token and slice key.
pub struct OmnigentAdapter {
    conn: Arc<Connection>,
    /// Agent name → its resolution, refreshed on a miss.
    agents: Mutex<HashMap<String, AgentRef>>,
    /// Who every session is shared with.
    shares: Vec<SessionShare>,
}

/// How the adapter reaches the server, shared with its sessions' streams.
struct Connection {
    base_url: Url,
    http: reqwest::Client,
    token: Option<Arc<dyn TokenSource>>,
    /// Whether session calls carry their host's slice key.
    slice_keys: bool,
    /// A client per host whose calls carry its slice key, as a default header.
    keyed: Mutex<HashMap<String, reqwest::Client>>,
    sessions: SliceKeys,
}

impl OmnigentAdapter {
    /// The server at `base_url` (with any gateway prefix: hosted Omnigent's
    /// API is `<workspace>/api/2.0/omnigent`), without a token, sending slice
    /// keys.
    pub fn new(base_url: Url) -> Self {
        Self {
            conn: Arc::new(Connection {
                base_url,
                http: reqwest::Client::new(),
                token: None,
                slice_keys: true,
                keyed: Mutex::new(HashMap::new()),
                sessions: SliceKeys::new(),
            }),
            agents: Mutex::new(HashMap::new()),
            shares: Vec::new(),
        }
    }

    fn conn_mut(&mut self) -> &mut Connection {
        Arc::get_mut(&mut self.conn).expect("configured before use")
    }

    /// Send `token`'s bearer token on every call, asked for per call.
    #[must_use]
    pub fn with_token(mut self, token: Arc<dyn TokenSource>) -> Self {
        self.conn_mut().token = Some(token);
        self
    }

    /// Send `token` on every call.
    #[must_use]
    pub fn with_static_token(self, token: impl Into<String>) -> Self {
        self.with_token(Arc::new(StaticToken::new(token)))
    }

    /// Whether a session's calls carry its host's slice key (on by default).
    /// An unsharded server ignores the header.
    #[must_use]
    pub fn with_slice_keys(mut self, on: bool) -> Self {
        self.conn_mut().slice_keys = on;
        self
    }

    /// Share every session [`OmnigentDriver::create_or_find`] returns with `shares`, at
    /// least at each one's level: the ones it creates, and the ones it finds, so sessions
    /// from before a restart or a change of the list are shared too.
    #[must_use]
    pub fn with_shares(mut self, shares: SessionShares) -> Self {
        self.shares = shares.0;
        self
    }

    /// The server's root.
    pub fn base_url(&self) -> &Url {
        &self.conn.base_url
    }

    async fn client(&self, slice_key: Option<&str>) -> Result<OmnigentClient> {
        self.conn.client(slice_key).await
    }

    async fn session_client(&self, session: &str) -> Result<OmnigentClient> {
        self.conn.session_client(session).await
    }

    /// Apply the shares to `session`. A grant that fails (the server's sharing is off, a
    /// policy refuses it) is logged and left for the next `create_or_find`: the session
    /// runs either way.
    async fn share(&self, session: &str) {
        if self.shares.is_empty() {
            return;
        }
        let client = match self.session_client(session).await {
            Ok(client) => client,
            Err(e) => {
                tracing::warn!(session, "sharing the session failed: {e}");
                return;
            }
        };
        for share in &self.shares {
            if let Err(e) = client
                .grant_session(session, &share.principal, share.level)
                .await
            {
                tracing::warn!(
                    session,
                    principal = %share.principal,
                    level = %share.level,
                    "sharing the session failed: {e}"
                );
            }
        }
    }

    async fn list_agents(&self, scope: Option<&str>) -> Result<Vec<api::Agent>> {
        let client = self.client(None).await?;
        let mut out = Vec::new();
        let mut after = None;
        loop {
            let page = client
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

/// [`SLICE_KEY_HEADER`] in the lower case `HeaderName::from_static` wants.
const SLICE_KEY_LOWER: &str = "x-databricks-omnigent-slice-key";
const _: () = assert!(SLICE_KEY_LOWER.len() == SLICE_KEY_HEADER.len());

impl Connection {
    /// A client with the current token, and `slice_key` on every call.
    async fn client(&self, slice_key: Option<&str>) -> Result<OmnigentClient> {
        let http = match slice_key.filter(|_| self.slice_keys) {
            Some(host) => self.keyed(host)?,
            None => self.http.clone(),
        };
        let mut transport = HttpClient::new(http);
        if let Some(source) = &self.token {
            transport = transport.with_token(source.token().await?);
        }
        Ok(OmnigentClient::with_transport(
            transport,
            self.base_url.clone(),
        ))
    }

    fn keyed(&self, host: &str) -> Result<reqwest::Client> {
        let mut keyed = self.keyed.lock().expect("keyed clients lock");
        if let Some(http) = keyed.get(host) {
            return Ok(http.clone());
        }
        let value = reqwest::header::HeaderValue::from_str(host)
            .map_err(|_| Error::internal(format!("host id {host:?} isn't a header value")))?;
        let http = reqwest::Client::builder()
            .default_headers(reqwest::header::HeaderMap::from_iter([(
                reqwest::header::HeaderName::from_static(SLICE_KEY_LOWER),
                value,
            )]))
            .build()
            .map_err(|e| Error::internal(format!("omnigent client: {e}")))?;
        keyed.insert(host.to_owned(), http.clone());
        Ok(http)
    }

    /// The client for calls on `session`: with its host's slice key, read
    /// from the session's record when this process hasn't seen it (a call
    /// any replica answers).
    async fn session_client(&self, session: &str) -> Result<OmnigentClient> {
        let key = match self.sessions.get(session) {
            Some(key) => key,
            None => {
                let record = self
                    .client(None)
                    .await?
                    .sessions()
                    .get_session(&api::GetSessionRequest {
                        session_id: session.to_owned(),
                        ..Default::default()
                    })
                    .await
                    .map_err(map_err)?;
                self.sessions.record(session, record.host_id.as_deref());
                record.host_id.filter(|h| !h.is_empty())
            }
        };
        self.client(key.as_deref()).await
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
            .client(None)
            .await?
            .hosts()
            .list_hosts(&api::ListHostsRequest::default())
            .await
            .map_err(map_err)?;
        Ok(list.hosts.iter().map(host_info).collect())
    }

    async fn create_or_find(&self, request: &SessionRequest) -> Result<Created> {
        let found = self
            .client(None)
            .await?
            .find_sessions(&request.labels.identity(), true)
            .await
            .map_err(map_err)?;
        if let Some(existing) = found.first() {
            self.conn
                .sessions
                .record(&existing.id, existing.host_id.as_deref());
            self.share(&existing.id).await;
            return Ok(Created {
                session: self.snapshot(&existing.id).await?,
                created: false,
            });
        }
        let first = initial_message(&request.prompt)?;
        let session = self
            .client(request.target.host_id())
            .await?
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
                r.mcp_registry_services = request.mcp_registry_services.clone();
                r.initial_items = vec![first];
            })
            .await
            .map_err(map_err)?;
        let host = session.host_id.as_deref().or(request.target.host_id());
        self.conn.sessions.record(&session.id, host);
        self.share(&session.id).await;
        Ok(Created {
            session: snapshot(&session),
            created: true,
        })
    }

    async fn snapshot(&self, session: &str) -> Result<SessionSnapshot> {
        let s = self
            .session_client(session)
            .await?
            .snapshot(session)
            .await
            .map_err(map_err)?;
        if s.host_id.as_deref().is_some_and(|h| !h.is_empty()) {
            self.conn.sessions.record(session, s.host_id.as_deref());
        }
        Ok(snapshot(&s))
    }

    async fn pull_requests(&self, session: &str) -> Result<DetectedPrs> {
        let (prs, synced) = self
            .session_client(session)
            .await?
            .pull_requests(session)
            .await
            .map_err(map_err)?;
        Ok(DetectedPrs {
            prs: prs.iter().map(detected).collect(),
            synced,
        })
    }

    async fn send(&self, session: &str, text: &str) -> Result<()> {
        self.session_client(session)
            .await?
            .send_message(session, text)
            .await
            .map(drop)
            .map_err(map_err)
    }

    async fn interrupt(&self, session: &str) -> Result<()> {
        self.session_client(session)
            .await?
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
        self.session_client(session)
            .await?
            .resolve_elicitation(session, elicitation_id, action, content)
            .await
            .map(drop)
            .map_err(map_err)
    }

    async fn events(&self, session: &str) -> Result<BoxStream<'static, DriverEvent>> {
        let stream = self
            .session_client(session)
            .await?
            .stream(session)
            .await
            .map_err(map_err)?;
        Ok(follow(stream, self.conn.clone(), session.to_owned()))
    }
}

/// The factory's events from a session's stream: the stream's own, plus the
/// session's PRs whenever it goes idle (read with the token current then),
/// and [`DriverEvent::Disconnected`] when the stream ends or fails.
fn follow<S>(stream: S, conn: Arc<Connection>, session: String) -> BoxStream<'static, DriverEvent>
where
    S: futures::Stream<Item = omnigent_client::Result<Event>> + Send + Unpin + 'static,
{
    struct St<S> {
        stream: Option<S>,
        ready: VecDeque<DriverEvent>,
        conn: Arc<Connection>,
        session: String,
    }
    let state = St {
        stream: Some(stream),
        ready: VecDeque::new(),
        conn,
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
                        let prs = match st.conn.session_client(&st.session).await {
                            Ok(client) => client.pull_requests(&st.session).await.map_err(map_err),
                            Err(e) => Err(e),
                        };
                        match prs {
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
    fn shares_parse() {
        let shares: SessionShares = " data-eng=read, alice@example.com=Manage ,"
            .parse()
            .unwrap();
        assert_eq!(
            shares.0,
            [
                SessionShare {
                    principal: "data-eng".into(),
                    level: PermissionLevel::Read,
                },
                SessionShare {
                    principal: "alice@example.com".into(),
                    level: PermissionLevel::Manage,
                },
            ]
        );
        assert_eq!(
            "".parse::<SessionShares>().unwrap(),
            SessionShares::default()
        );
        for bad in ["data-eng", "=read", "data-eng=owner", "a=read,a=edit"] {
            assert!(bad.parse::<SessionShares>().is_err(), "{bad}");
        }
    }

    /// A fake Omnigent with `existing` sessions (by id, with the request's labels), that
    /// records the grants it gets and refuses those to `denied`.
    async fn sharing_omnigent(
        existing: Option<&'static str>,
    ) -> (Url, std::sync::Arc<Mutex<Vec<Value>>>) {
        use axum::extract::{Json, Path, State};
        use axum::http::StatusCode;
        use axum::routing::{get, put};
        use serde_json::json;

        type Grants = std::sync::Arc<Mutex<Vec<Value>>>;
        fn session(id: &str) -> Value {
            json!({"id": id, "agent_id": "agent_1", "status": "running", "created_at": 1,
                   "labels": {"factory.run": "run-1", "factory.step": "1"}})
        }
        let grants = Grants::default();
        let list = move || async move {
            let data: Vec<Value> = existing.into_iter().map(session).collect();
            Json(json!({"data": data, "has_more": false}))
        };
        let app = axum::Router::new()
            .route(
                "/v1/sessions",
                get(list).post(|| async { Json(session("conv_new")) }),
            )
            .route(
                "/v1/sessions/{id}",
                get(|Path(id): Path<String>| async move { Json(session(&id)) }),
            )
            .route(
                "/v1/sessions/{id}/permissions",
                put(
                    |State(grants): State<Grants>,
                     Path(id): Path<String>,
                     Json(mut body): Json<Value>| async move {
                        if body["user_id"] == "denied" {
                            let error = json!({"error": {"code": "forbidden", "message": "no"}});
                            return (StatusCode::FORBIDDEN, Json(error));
                        }
                        body["session_id"] = id.clone().into();
                        grants.lock().unwrap().push(body.clone());
                        let answer = json!({"user_id": body["user_id"], "conversation_id": id,
                                            "level": body["level"]});
                        (StatusCode::OK, Json(answer))
                    },
                ),
            )
            .with_state(grants.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url.parse().unwrap(), grants)
    }

    fn share_request() -> SessionRequest {
        SessionRequest {
            agent_id: "agent_1".into(),
            title: "task".into(),
            labels: crate::driver::SessionLabels {
                run: "run-1".into(),
                step: 1,
                ..Default::default()
            },
            target: Target::Host {
                host_id: "host_1".into(),
                workspace: "/work".into(),
            },
            branch: None,
            base_branch: None,
            harness: None,
            model: None,
            reasoning_effort: None,
            mcp_registry_services: Vec::new(),
            prompt: "go".into(),
        }
    }

    #[tokio::test]
    async fn sessions_are_shared_when_created_and_when_found() {
        let shares: SessionShares = "data-eng=read,denied=edit,alice@example.com=manage"
            .parse()
            .unwrap();
        for (existing, id, created) in [
            (None, "conv_new", true),
            (Some("conv_old"), "conv_old", false),
        ] {
            let (url, grants) = sharing_omnigent(existing).await;
            let adapter = OmnigentAdapter::new(url).with_shares(shares.clone());
            // A refused grant doesn't fail the dispatch.
            let found = adapter.create_or_find(&share_request()).await.unwrap();
            assert_eq!((found.session.id.as_str(), found.created), (id, created));
            assert_eq!(
                *grants.lock().unwrap(),
                [
                    serde_json::json!({"session_id": id, "user_id": "data-eng", "level": 1,
                                       "ensure_minimum": true}),
                    serde_json::json!({"session_id": id, "user_id": "alice@example.com",
                                       "level": 3, "ensure_minimum": true}),
                ]
            );
        }
    }

    #[tokio::test]
    async fn no_shares_no_grants() {
        let (url, grants) = sharing_omnigent(None).await;
        let adapter = OmnigentAdapter::new(url);
        adapter.create_or_find(&share_request()).await.unwrap();
        assert!(grants.lock().unwrap().is_empty());
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

    /// A request the fake server saw: method and path, slice key, bearer.
    type Seen = (String, Option<String>, Option<String>);

    /// An Omnigent that answers sessions calls and records each request.
    async fn fake_omnigent() -> (Url, Arc<Mutex<Vec<Seen>>>) {
        use axum::extract::{Path, State};
        use axum::http::{HeaderMap, Method, Uri};
        use axum::routing::{get, post};
        use serde_json::json;

        type Log = Arc<Mutex<Vec<Seen>>>;
        fn record(log: &Log, method: &Method, uri: &Uri, headers: &HeaderMap) {
            let header = |name: &str| {
                headers
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned)
            };
            log.lock().unwrap().push((
                format!("{method} {}", uri.path()),
                header(SLICE_KEY_LOWER),
                header("authorization"),
            ));
        }
        fn session(id: &str, host: &str) -> axum::Json<Value> {
            axum::Json(json!({
                "id": id, "agent_id": "ag_x", "status": "running",
                "created_at": 0, "archived": false, "labels": {}, "host_id": host,
            }))
        }
        let log: Log = Arc::default();
        let app = axum::Router::new()
            .route(
                "/omnigent/v1/sessions",
                get(
                    |State(log): State<Log>, m: Method, u: Uri, h: HeaderMap| async move {
                        record(&log, &m, &u, &h);
                        axum::Json(json!({"data": [], "has_more": false}))
                    },
                )
                .post(
                    |State(log): State<Log>, m: Method, u: Uri, h: HeaderMap| async move {
                        record(&log, &m, &u, &h);
                        session("s-new", "h1")
                    },
                ),
            )
            .route(
                "/omnigent/v1/sessions/{id}",
                get(
                    |State(log): State<Log>,
                     Path(id): Path<String>,
                     m: Method,
                     u: Uri,
                     h: HeaderMap| async move {
                        record(&log, &m, &u, &h);
                        session(&id, "h2")
                    },
                ),
            )
            .route(
                "/omnigent/v1/sessions/{id}/events",
                post(
                    |State(log): State<Log>, m: Method, u: Uri, h: HeaderMap| async move {
                        record(&log, &m, &u, &h);
                        axum::Json(json!({"queued": false}))
                    },
                ),
            )
            .with_state(log.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let url = format!("http://{addr}/omnigent").parse().unwrap();
        (url, log)
    }

    /// A token source that hands out `t1`, `t2`, …: one per call.
    struct Counting(std::sync::atomic::AtomicUsize);

    #[async_trait]
    impl TokenSource for Counting {
        async fn token(&self) -> Result<String> {
            let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            Ok(format!("t{n}"))
        }
    }

    fn request_on(host: &str) -> SessionRequest {
        SessionRequest {
            agent_id: "ag_x".into(),
            title: "t".into(),
            labels: super::super::SessionLabels {
                graph: "g".into(),
                node: "n".into(),
                run: "r".into(),
                step: 0,
                revision: "h".into(),
            },
            target: Target::Host {
                host_id: host.into(),
                workspace: "/w".into(),
            },
            branch: None,
            base_branch: None,
            harness: None,
            model: None,
            reasoning_effort: None,
            mcp_registry_services: Vec::new(),
            prompt: "go".into(),
        }
    }

    #[tokio::test]
    async fn session_calls_carry_the_hosts_slice_key_and_a_current_token() {
        let (url, log) = fake_omnigent().await;
        let adapter = OmnigentAdapter::new(url).with_token(Arc::new(Counting(Default::default())));

        let created = adapter.create_or_find(&request_on("h1")).await.unwrap();
        assert!(created.created);
        adapter.send("s-new", "more").await.unwrap();
        // A session this process hasn't seen: its host is read first.
        adapter.send("s-old", "hi").await.unwrap();
        adapter.interrupt("s-old").await.unwrap();

        let seen = log.lock().unwrap().clone();
        let key = |k: &str| Some(k.to_owned());
        let bearer = |n: u32| Some(format!("Bearer t{n}"));
        assert_eq!(
            seen,
            [
                ("GET /omnigent/v1/sessions".into(), None, bearer(1)),
                ("POST /omnigent/v1/sessions".into(), key("h1"), bearer(2)),
                (
                    "POST /omnigent/v1/sessions/s-new/events".into(),
                    key("h1"),
                    bearer(3)
                ),
                ("GET /omnigent/v1/sessions/s-old".into(), None, bearer(4)),
                (
                    "POST /omnigent/v1/sessions/s-old/events".into(),
                    key("h2"),
                    bearer(5)
                ),
                (
                    "POST /omnigent/v1/sessions/s-old/events".into(),
                    key("h2"),
                    bearer(6)
                ),
            ]
        );
    }

    #[tokio::test]
    async fn slice_keys_can_be_turned_off() {
        let (url, log) = fake_omnigent().await;
        let adapter = OmnigentAdapter::new(url)
            .with_static_token("tok")
            .with_slice_keys(false);
        adapter.create_or_find(&request_on("h1")).await.unwrap();
        adapter.send("s-new", "more").await.unwrap();
        let seen = log.lock().unwrap().clone();
        assert_eq!(seen.len(), 3);
        assert!(seen.iter().all(|(_, key, _)| key.is_none()), "{seen:?}");
        assert!(
            seen.iter()
                .all(|(_, _, auth)| auth.as_deref() == Some("Bearer tok"))
        );
    }

    #[test]
    fn the_slice_key_name_is_the_header_lowercased() {
        assert_eq!(SLICE_KEY_LOWER, SLICE_KEY_HEADER.to_ascii_lowercase());
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
