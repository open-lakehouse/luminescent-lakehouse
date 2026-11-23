//! A deterministic [`OmnigentDriver`] for tests: sessions in memory, ids in
//! order (`sess-1`, `sess-2`, …), and every transition scripted by the test.
//!
//! ```
//! # use factory_core::driver::fake::FakeDriver;
//! let fake = FakeDriver::new();
//! // … the reconciler creates sess-1 …
//! # fake.seed_session("sess-1", Default::default());
//! fake.turn("sess-1");
//! fake.open_pr("sess-1", "https://github.com/o/r/pull/1");
//! fake.idle("sess-1");
//! ```
//!
//! Any agent name resolves (to `ag_<name>`, version `1`, unless
//! [`FakeDriver::with_agent`] says otherwise), and there's one online host,
//! `local`, configured for `claude-sdk` and `codex`, unless
//! [`FakeDriver::with_hosts`] replaces it.
//!
//! The fake keeps [`SliceKeys`] as the adapter does, and records the slice
//! key each session call would carry ([`FakeDriver::routed`]).

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::stream::BoxStream;
use serde_json::{Map, Value};

use super::{
    AgentRef, Ask, AskAction, Created, DetectedPr, DetectedPrs, DriverEvent, HarnessState,
    HostInfo, OmnigentDriver, SessionLabels, SessionRequest, SessionSnapshot, SessionStatus,
    SliceKeys,
};
use crate::{Error, Result};

/// A call the driver received, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Create {
        session: String,
        request: Box<SessionRequest>,
    },
    Found {
        session: String,
    },
    Send {
        session: String,
        text: String,
    },
    Interrupt {
        session: String,
    },
    Resolve {
        session: String,
        elicitation_id: String,
        action: AskAction,
    },
}

/// A session call, with the slice key it carried ([`super::SLICE_KEY_HEADER`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed {
    /// The trait method: `snapshot`, `pull_requests`, `send`, `interrupt`,
    /// `resolve_ask` or `events`.
    pub call: &'static str,
    pub session: String,
    pub slice_key: Option<String>,
}

/// A session the fake holds.
#[derive(Debug, Clone, Default)]
pub struct FakeSession {
    pub request: Option<SessionRequest>,
    pub snapshot: SessionSnapshot,
    pub prs: DetectedPrs,
    pub sent: Vec<String>,
}

#[derive(Default)]
struct State {
    agents: BTreeMap<String, AgentRef>,
    hosts: Vec<HostInfo>,
    sessions: BTreeMap<String, FakeSession>,
    next: u32,
    calls: Vec<Call>,
    routes: Vec<Routed>,
    watchers: BTreeMap<String, Vec<UnboundedSender<DriverEvent>>>,
    /// Fail the next create *after* the session exists: a crash between
    /// the create and its record.
    crash_after_create: bool,
}

/// See the [module docs](self).
pub struct FakeDriver {
    state: Mutex<State>,
    slices: SliceKeys,
}

impl Default for FakeDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeDriver {
    pub fn new() -> Self {
        let local = HostInfo {
            id: "host-local".into(),
            name: "local".into(),
            online: true,
            sandbox_provider: None,
            harnesses: BTreeMap::from([
                ("claude-sdk".into(), HarnessState::Ready),
                ("codex".into(), HarnessState::Ready),
            ]),
        };
        Self {
            state: Mutex::new(State {
                hosts: vec![local],
                ..Default::default()
            }),
            slices: SliceKeys::new(),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.lock().expect("fake driver lock"))
    }

    /// Resolve `agent.name` to `agent`.
    #[must_use]
    pub fn with_agent(self, agent: AgentRef) -> Self {
        self.with(|s| s.agents.insert(agent.name.clone(), agent));
        self
    }

    /// Replace the hosts.
    #[must_use]
    pub fn with_hosts(self, hosts: Vec<HostInfo>) -> Self {
        self.with(|s| s.hosts = hosts);
        self
    }

    /// Make the next create fail after creating the session.
    pub fn crash_after_next_create(&self) {
        self.with(|s| s.crash_after_create = true);
    }

    /// Add a session that exists already (a demo's, or one from before a
    /// restart).
    pub fn seed_session(&self, id: &str, labels: SessionLabels) {
        self.with(|s| {
            s.sessions.insert(
                id.into(),
                FakeSession {
                    snapshot: SessionSnapshot {
                        id: id.into(),
                        status: SessionStatus::Running,
                        labels: labels.all(),
                        turns: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
        });
    }

    /// Every session call so far, with its slice key.
    pub fn routed(&self) -> Vec<Routed> {
        self.with(|s| s.routes.clone())
    }

    /// Forget which host each session is on, as a restarted driver would:
    /// the next call for a session reads it from the session.
    pub fn forget_slice_keys(&self) {
        self.slices.clear();
    }

    /// The slice key a call for `session` carries: the one recorded, or the
    /// session's own host (the server's record of it), recorded now.
    fn route(&self, call: &'static str, session: &str) {
        let slice_key = match self.slices.get(session) {
            Some(key) => key,
            None => {
                let known = self.with(|s| {
                    s.sessions.get(session).map(|x| {
                        x.request
                            .as_ref()
                            .and_then(|r| r.target.host_id())
                            .map(str::to_owned)
                    })
                });
                if let Some(host) = &known {
                    self.slices.record(session, host.as_deref());
                }
                known.flatten()
            }
        };
        self.with(|s| {
            s.routes.push(Routed {
                call,
                session: session.into(),
                slice_key,
            })
        });
    }

    /// Every call so far.
    pub fn calls(&self) -> Vec<Call> {
        self.with(|s| s.calls.clone())
    }

    /// The sessions created so far (not found, not seeded).
    pub fn created(&self) -> Vec<SessionRequest> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Create { request, .. } => Some(*request),
                _ => None,
            })
            .collect()
    }

    pub fn session(&self, id: &str) -> Option<FakeSession> {
        self.with(|s| s.sessions.get(id).cloned())
    }

    /// The sessions whose labels include all of `labels`.
    pub fn sessions_with(&self, labels: &BTreeMap<String, String>) -> Vec<String> {
        self.with(|s| find(&s.sessions, labels))
    }

    fn emit(&self, id: &str, events: Vec<DriverEvent>) {
        self.with(|s| {
            if let Some(ws) = s.watchers.get_mut(id) {
                ws.retain(|w| events.iter().all(|e| w.unbounded_send(e.clone()).is_ok()));
            }
        });
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut FakeSession)) {
        self.with(|s| f(s.sessions.get_mut(id).expect("no such fake session")));
    }

    /// A turn starts: the session runs.
    pub fn turn(&self, id: &str) {
        self.update(id, |s| {
            s.snapshot.turns += 1;
            s.snapshot.status = SessionStatus::Running;
        });
        self.emit(
            id,
            vec![
                DriverEvent::Status(SessionStatus::Running),
                DriverEvent::TurnStarted,
            ],
        );
    }

    /// Omnigent detects a PR the session opened (synced).
    pub fn open_pr(&self, id: &str, url: &str) {
        let (repo, number) = parse_pr_url(url);
        self.update(id, |s| {
            s.prs.synced = true;
            s.prs.prs.push(DetectedPr {
                url: url.into(),
                repo,
                number,
                state: "open".into(),
                relationship: "created".into(),
                ..Default::default()
            });
        });
    }

    /// The turn ends: the session is idle, and its PRs are sent along.
    pub fn idle(&self, id: &str) {
        let prs = self.with(|s| {
            let session = s.sessions.get_mut(id).expect("no such fake session");
            session.snapshot.status = SessionStatus::Idle;
            session.prs.synced = true;
            session.prs.clone()
        });
        self.emit(
            id,
            vec![
                DriverEvent::Status(SessionStatus::Idle),
                DriverEvent::PullRequests(prs),
            ],
        );
    }

    /// The session asks a person something.
    pub fn ask(&self, id: &str, elicitation_id: &str, message: &str) {
        let ask = Ask {
            elicitation_id: elicitation_id.into(),
            message: message.into(),
            tool: String::new(),
        };
        self.update(id, |s| {
            s.snapshot.status = SessionStatus::Waiting;
            s.snapshot.asks.push(ask.clone());
        });
        self.emit(
            id,
            vec![
                DriverEvent::Status(SessionStatus::Waiting),
                DriverEvent::Ask(ask),
            ],
        );
    }

    /// The session fails.
    pub fn fail(&self, id: &str, message: &str) {
        self.update(id, |s| {
            s.snapshot.status = SessionStatus::Failed;
            s.snapshot.last_error = BTreeMap::from([("message".into(), message.into())]);
        });
        self.emit(
            id,
            vec![
                DriverEvent::TurnFailed(message.into()),
                DriverEvent::Status(SessionStatus::Failed),
            ],
        );
    }

    /// The session's spend so far.
    pub fn cost(&self, id: &str, usd: f64) {
        self.update(id, |s| s.snapshot.cost_usd = usd);
    }

    /// Drop every stream of the session.
    pub fn disconnect(&self, id: &str) {
        self.emit(id, vec![DriverEvent::Disconnected]);
        self.with(|s| s.watchers.remove(id));
    }

    /// Forget a session, as if it was deleted on the server.
    pub fn remove(&self, id: &str) {
        self.disconnect(id);
        self.with(|s| s.sessions.remove(id));
    }
}

impl FakeDriver {
    fn create_or_find_inner(&self, request: &SessionRequest) -> Result<Created> {
        self.with(|s| {
            if let Some(id) = find(&s.sessions, &request.labels.identity())
                .into_iter()
                .next()
            {
                s.calls.push(Call::Found {
                    session: id.clone(),
                });
                return Ok(Created {
                    session: s.sessions[&id].snapshot.clone(),
                    created: false,
                });
            }
            s.next += 1;
            let id = format!("sess-{}", s.next);
            let session = FakeSession {
                request: Some(request.clone()),
                snapshot: SessionSnapshot {
                    id: id.clone(),
                    status: SessionStatus::Launching,
                    labels: request.labels.all(),
                    harness: request.harness.clone(),
                    branch: request.branch.clone(),
                    ..Default::default()
                },
                sent: vec![request.prompt.clone()],
                ..Default::default()
            };
            let snapshot = session.snapshot.clone();
            s.sessions.insert(id.clone(), session);
            s.calls.push(Call::Create {
                session: id,
                request: Box::new(request.clone()),
            });
            if std::mem::take(&mut s.crash_after_create) {
                return Err(Error::internal("the fake crashed after the create"));
            }
            Ok(Created {
                session: snapshot,
                created: true,
            })
        })
    }
}

fn find(
    sessions: &BTreeMap<String, FakeSession>,
    labels: &BTreeMap<String, String>,
) -> Vec<String> {
    sessions
        .iter()
        .filter(|(_, s)| {
            labels
                .iter()
                .all(|(k, v)| s.snapshot.labels.get(k).is_some_and(|have| have == v))
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// `https://github.com/<owner>/<name>/pull/<n>` → (`owner/name`, n).
fn parse_pr_url(url: &str) -> (String, u32) {
    let parts: Vec<&str> = url.trim_end_matches('/').rsplit('/').collect();
    match parts.as_slice() {
        [n, "pull", name, owner, ..] => (format!("{owner}/{name}"), n.parse().unwrap_or(0)),
        _ => (String::new(), 0),
    }
}

fn not_found(id: &str) -> Error {
    Error::NotFound(format!("session {id}"))
}

#[async_trait]
impl OmnigentDriver for FakeDriver {
    async fn resolve_agent(&self, name: &str) -> Result<AgentRef> {
        Ok(self.with(|s| {
            s.agents.get(name).cloned().unwrap_or_else(|| AgentRef {
                id: format!("ag_{name}"),
                name: name.into(),
                version: "1".into(),
                harness: None,
            })
        }))
    }

    async fn hosts(&self) -> Result<Vec<HostInfo>> {
        Ok(self.with(|s| s.hosts.clone()))
    }

    async fn create_or_find(&self, request: &SessionRequest) -> Result<Created> {
        let created = self.create_or_find_inner(request);
        if let Ok(c) = &created {
            let host = self.with(|s| {
                s.sessions
                    .get(&c.session.id)
                    .and_then(|x| x.request.as_ref())
                    .and_then(|r| r.target.host_id())
                    .map(str::to_owned)
            });
            self.slices.record(&c.session.id, host.as_deref());
        }
        created
    }

    async fn snapshot(&self, session: &str) -> Result<SessionSnapshot> {
        self.route("snapshot", session);
        self.with(|s| {
            s.sessions
                .get(session)
                .map(|x| x.snapshot.clone())
                .ok_or_else(|| not_found(session))
        })
    }

    async fn pull_requests(&self, session: &str) -> Result<DetectedPrs> {
        self.route("pull_requests", session);
        self.with(|s| {
            s.sessions
                .get(session)
                .map(|x| x.prs.clone())
                .ok_or_else(|| not_found(session))
        })
    }

    async fn send(&self, session: &str, text: &str) -> Result<()> {
        self.route("send", session);
        self.with(|s| {
            let x = s
                .sessions
                .get_mut(session)
                .ok_or_else(|| not_found(session))?;
            x.sent.push(text.into());
            s.calls.push(Call::Send {
                session: session.into(),
                text: text.into(),
            });
            Ok(())
        })
    }

    async fn interrupt(&self, session: &str) -> Result<()> {
        self.route("interrupt", session);
        self.with(|s| {
            s.sessions.get(session).ok_or_else(|| not_found(session))?;
            s.calls.push(Call::Interrupt {
                session: session.into(),
            });
            Ok(())
        })
    }

    async fn resolve_ask(
        &self,
        session: &str,
        elicitation_id: &str,
        action: AskAction,
        _content: Option<Map<String, Value>>,
    ) -> Result<()> {
        self.route("resolve_ask", session);
        self.with(|s| {
            let x = s
                .sessions
                .get_mut(session)
                .ok_or_else(|| not_found(session))?;
            x.snapshot
                .asks
                .retain(|a| a.elicitation_id != elicitation_id);
            s.calls.push(Call::Resolve {
                session: session.into(),
                elicitation_id: elicitation_id.into(),
                action,
            });
            Ok(())
        })
    }

    async fn events(&self, session: &str) -> Result<BoxStream<'static, DriverEvent>> {
        self.route("events", session);
        let (tx, rx) = unbounded();
        self.with(|s| {
            s.sessions.get(session).ok_or_else(|| not_found(session))?;
            s.watchers.entry(session.into()).or_default().push(tx);
            Ok::<_, Error>(())
        })?;
        Ok(rx.boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Target;

    fn request(run: &str, step: u32) -> SessionRequest {
        SessionRequest {
            agent_id: "ag_executor".into(),
            title: "t".into(),
            labels: SessionLabels {
                graph: "g".into(),
                node: "n".into(),
                run: run.into(),
                step,
                revision: "h".into(),
            },
            target: Target::Host {
                host_id: "host-local".into(),
                workspace: "/workspace".into(),
            },
            branch: None,
            base_branch: None,
            harness: None,
            model: None,
            reasoning_effort: None,
            mcp_registry_services: Vec::new(),
            prompt: "do it".into(),
        }
    }

    #[tokio::test]
    async fn creates_once_per_run_and_step() {
        let fake = FakeDriver::new();
        let a = fake.create_or_find(&request("r1", 0)).await.unwrap();
        assert!(a.created);
        assert_eq!(a.session.id, "sess-1");
        let again = fake.create_or_find(&request("r1", 0)).await.unwrap();
        assert!(!again.created);
        assert_eq!(again.session.id, "sess-1");
        let b = fake.create_or_find(&request("r1", 1)).await.unwrap();
        assert_eq!(b.session.id, "sess-2");
        assert_eq!(fake.created().len(), 2);
        assert_eq!(fake.session("sess-1").unwrap().sent, ["do it"]);
    }

    #[tokio::test]
    async fn session_calls_carry_their_hosts_slice_key() {
        let fake = FakeDriver::new();
        let on_host = fake.create_or_find(&request("r1", 0)).await.unwrap();
        let mut managed = request("r2", 0);
        managed.target = Target::Managed {
            provider: "lakebox".into(),
            sandbox_profile: String::new(),
            workspace: "https://github.com/o/r".into(),
        };
        let in_sandbox = fake.create_or_find(&managed).await.unwrap();
        let (a, b) = (on_host.session.id.as_str(), in_sandbox.session.id.as_str());

        fake.snapshot(a).await.unwrap();
        fake.send(a, "more").await.unwrap();
        let _events = fake.events(a).await.unwrap();
        fake.snapshot(b).await.unwrap();
        // After a restart the key comes from the session itself.
        fake.forget_slice_keys();
        fake.interrupt(a).await.unwrap();
        fake.pull_requests(b).await.unwrap();

        let routed = fake.routed();
        let keys: Vec<(&str, &str, Option<&str>)> = routed
            .iter()
            .map(|r| {
                let which = if r.session == a { "a" } else { "b" };
                (r.call, which, r.slice_key.as_deref())
            })
            .collect();
        assert_eq!(
            keys,
            [
                ("snapshot", "a", Some("host-local")),
                ("send", "a", Some("host-local")),
                ("events", "a", Some("host-local")),
                ("snapshot", "b", None),
                ("interrupt", "a", Some("host-local")),
                ("pull_requests", "b", None),
            ]
        );
    }

    #[tokio::test]
    async fn a_crash_after_create_leaves_the_session_to_find() {
        let fake = FakeDriver::new();
        fake.crash_after_next_create();
        assert!(fake.create_or_find(&request("r1", 0)).await.is_err());
        let found = fake.create_or_find(&request("r1", 0)).await.unwrap();
        assert!(!found.created);
        assert_eq!(
            fake.sessions_with(&request("r1", 0).labels.identity())
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn scripted_events_stream() {
        let fake = FakeDriver::new();
        let id = fake
            .create_or_find(&request("r", 0))
            .await
            .unwrap()
            .session
            .id;
        let mut events = fake.events(&id).await.unwrap();
        fake.turn(&id);
        fake.open_pr(&id, "https://github.com/o/r/pull/7");
        fake.idle(&id);
        fake.ask(&id, "e1", "may I?");
        fake.disconnect(&id);
        let got: Vec<DriverEvent> = events.by_ref().collect().await;
        assert_eq!(got[0], DriverEvent::Status(SessionStatus::Running));
        assert_eq!(got[1], DriverEvent::TurnStarted);
        assert_eq!(got[2], DriverEvent::Status(SessionStatus::Idle));
        let DriverEvent::PullRequests(prs) = &got[3] else {
            panic!("{got:?}")
        };
        assert_eq!((prs.prs[0].repo.as_str(), prs.prs[0].number), ("o/r", 7));
        assert!(matches!(&got[5], DriverEvent::Ask(a) if a.elicitation_id == "e1"));
        assert_eq!(got.last(), Some(&DriverEvent::Disconnected));
        fake.resolve_ask(&id, "e1", AskAction::Accept, None)
            .await
            .unwrap();
        assert!(fake.snapshot(&id).await.unwrap().asks.is_empty());
    }
}
