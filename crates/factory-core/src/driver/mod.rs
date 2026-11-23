//! The factory's one way to Omnigent: the [`OmnigentDriver`] trait.
//!
//! The reconciler starts and follows sessions only through this trait, so its
//! tests run on [`fake::FakeDriver`] and production on
//! [`omnigent::OmnigentAdapter`] (feature `omnigent`), over
//! `omnigent-client`.
//!
//! What the trait models, from Omnigent 0.22:
//!
//! - **Create-or-find.** Omnigent has no idempotency key on create, so every
//!   session carries the `factory.*` labels ([`SessionLabels`]), and
//!   [`OmnigentDriver::create_or_find`] looks one up by its run and step
//!   before creating. A crash between the create and the record then can't
//!   start a second session. The lookup pages through the caller's sessions,
//!   since the server has no label filter.
//! - **The prompt goes with the create** (as the session's initial item), so
//!   a found session never needs it sent again.
//! - **The stream.** A turn starts with `response.in_progress`. The stream
//!   never ends and has no replay, so a step is done when the session's
//!   status is `idle` after its turn started, and after a restart (or a
//!   dropped connection) the state comes from [`OmnigentDriver::snapshot`],
//!   which counts the turns so far ([`SessionSnapshot::turns`]).
//! - **PRs** aren't on the stream or the snapshot. They come from
//!   [`OmnigentDriver::pull_requests`]; the adapter adds a
//!   [`DriverEvent::PullRequests`] whenever a turn ends.
//! - **Agents** resolve by name, the user's own agents (`scope=user`) first.
//! - **Hosts** say which harnesses they're configured for, which gates
//!   placement ([`HostInfo::check_harness`]).

use std::collections::BTreeMap;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde_json::{Map, Value};

use crate::Result;

#[cfg(any(test, feature = "testing"))]
pub mod fake;
#[cfg(feature = "omnigent")]
pub mod omnigent;

/// The label keys the factory stamps on every session.
pub mod label {
    pub const GRAPH: &str = "factory.graph";
    pub const NODE: &str = "factory.node";
    pub const RUN: &str = "factory.run";
    pub const STEP: &str = "factory.step";
    pub const REVISION: &str = "factory.revision";
}

/// Where a session belongs in the factory: its labels.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionLabels {
    pub graph: String,
    pub node: String,
    pub run: String,
    pub step: u32,
    pub revision: String,
}

impl SessionLabels {
    /// Every label, as stamped on the session.
    pub fn all(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (label::GRAPH.to_owned(), self.graph.clone()),
            (label::NODE.to_owned(), self.node.clone()),
            (label::RUN.to_owned(), self.run.clone()),
            (label::STEP.to_owned(), self.step.to_string()),
            (label::REVISION.to_owned(), self.revision.clone()),
        ])
    }

    /// The labels that identify the session: its run and step. A step has
    /// one session, whichever revision it was started against.
    pub fn identity(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (label::RUN.to_owned(), self.run.clone()),
            (label::STEP.to_owned(), self.step.to_string()),
        ])
    }
}

/// An agent, resolved from its spec name at dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRef {
    /// The server's id for it (`ag_…`).
    pub id: String,
    pub name: String,
    pub version: String,
    /// The spec's harness, when it names one.
    pub harness: Option<String>,
}

/// Whether a host can run a harness, from its `configured_harnesses`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessState {
    Ready,
    /// Configured off.
    Disabled,
    /// Not usable, and why (`needs-auth`, `binary-missing`).
    Unavailable(String),
}

/// A registered host, as placement sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostInfo {
    pub id: String,
    pub name: String,
    pub online: bool,
    /// Set for a managed sandbox provider's host.
    pub sandbox_provider: Option<String>,
    pub harnesses: BTreeMap<String, HarnessState>,
}

impl HostInfo {
    /// Whether the host matches a registry placement's `host`, by id or
    /// name.
    pub fn is(&self, host: &str) -> bool {
        self.id == host || self.name == host
    }

    /// Whether a session on `harness` can run here; the reason if not.
    pub fn check_harness(&self, harness: &str) -> std::result::Result<(), String> {
        if !self.online {
            return Err(format!("host {} is offline", self.name_or_id()));
        }
        match self.harnesses.get(harness) {
            Some(HarnessState::Ready) => Ok(()),
            Some(HarnessState::Disabled) => Err(format!(
                "host {} has harness {harness} turned off",
                self.name_or_id()
            )),
            Some(HarnessState::Unavailable(why)) => Err(format!(
                "host {} can't run harness {harness}: {why}",
                self.name_or_id()
            )),
            None => Err(format!(
                "host {} isn't configured for harness {harness}",
                self.name_or_id()
            )),
        }
    }

    fn name_or_id(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}

/// Where a session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A registered host (`host_type: external`), in an absolute workspace.
    Host { host_id: String, workspace: String },
    /// A managed sandbox. `workspace` is a git URL, optionally `#branch`.
    Managed {
        provider: String,
        sandbox_profile: String,
        workspace: String,
    },
}

/// Everything a session is created with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRequest {
    pub agent_id: String,
    pub title: String,
    pub labels: SessionLabels,
    pub target: Target,
    /// A new branch for the session's worktree, and what it branches from.
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    /// The server's MCP registry services the session may call, by id: an
    /// environment's egress routes, so the session holds no credential.
    pub mcp_registry_services: Vec<String>,
    /// The first user message, sent with the create.
    pub prompt: String,
}

/// A session's status.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SessionStatus {
    #[default]
    Launching,
    Running,
    /// Waiting on a person (an elicitation) or a client tool.
    Waiting,
    Idle,
    Failed,
    Other(String),
}

impl SessionStatus {
    pub fn parse(status: &str) -> Self {
        match status {
            "launching" | "" => Self::Launching,
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "idle" => Self::Idle,
            "failed" => Self::Failed,
            other => Self::Other(other.to_owned()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Launching => "launching",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Idle => "idle",
            Self::Failed => "failed",
            Self::Other(s) => s,
        }
    }
}

/// A pending question from the session to a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub elicitation_id: String,
    pub message: String,
    /// The policy or tool asking, when known.
    pub tool: String,
}

/// A session's state, from a snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionSnapshot {
    pub id: String,
    pub status: SessionStatus,
    pub labels: BTreeMap<String, String>,
    pub cost_usd: f64,
    /// Why the last task failed (`code`, `message`, …); empty otherwise.
    pub last_error: BTreeMap<String, String>,
    pub harness: Option<String>,
    pub branch: Option<String>,
    pub asks: Vec<Ask>,
    /// How many turns have started: a fresh session is idle before its
    /// first, which isn't "done", and a follow-up message is done only once
    /// the turn it started has ended.
    pub turns: u32,
}

impl SessionSnapshot {
    /// The last error as one line, empty when there's none.
    pub fn error_text(&self) -> String {
        match (self.last_error.get("code"), self.last_error.get("message")) {
            (Some(c), Some(m)) => format!("{c}: {m}"),
            (None, Some(m)) => m.clone(),
            (Some(c), None) => c.clone(),
            (None, None) => self
                .last_error
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// A PR Omnigent detected for a session.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DetectedPr {
    pub url: String,
    /// `owner/name`.
    pub repo: String,
    pub number: u32,
    pub title: String,
    /// `open`, `merged` or `closed`.
    pub state: String,
    pub draft: bool,
    /// `created`, `worked_on`, `attached` or `inferred`.
    pub relationship: String,
}

/// A session's PRs, and whether Omnigent has synced them from the forge. An
/// empty unsynced list means "not known yet", not "none".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DetectedPrs {
    pub prs: Vec<DetectedPr>,
    pub synced: bool,
}

/// What happens in a session, as the reconciler follows it.
#[derive(Debug, Clone, PartialEq)]
pub enum DriverEvent {
    /// A turn started (`response.in_progress`).
    TurnStarted,
    Status(SessionStatus),
    /// The turn ended in an error (`response.failed`).
    TurnFailed(String),
    Ask(Ask),
    PullRequests(DetectedPrs),
    /// The connection dropped. Reconnect, and resync from a snapshot.
    Disconnected,
}

/// How an [`Ask`] is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskAction {
    Accept,
    Decline,
    Cancel,
}

/// The created or found session.
#[derive(Debug, Clone, PartialEq)]
pub struct Created {
    pub session: SessionSnapshot,
    /// False when a session with the request's run and step already
    /// existed.
    pub created: bool,
}

/// The factory's view of Omnigent.
#[async_trait]
pub trait OmnigentDriver: Send + Sync + 'static {
    /// The agent named `name`: the user's own first, then the server's.
    async fn resolve_agent(&self, name: &str) -> Result<AgentRef>;
    async fn hosts(&self) -> Result<Vec<HostInfo>>;
    /// The session of the request's run and step, created if there's none.
    async fn create_or_find(&self, request: &SessionRequest) -> Result<Created>;
    async fn snapshot(&self, session: &str) -> Result<SessionSnapshot>;
    async fn pull_requests(&self, session: &str) -> Result<DetectedPrs>;
    /// A user message: starts a turn, or queues behind the running one.
    async fn send(&self, session: &str, text: &str) -> Result<()>;
    async fn interrupt(&self, session: &str) -> Result<()>;
    async fn resolve_ask(
        &self,
        session: &str,
        elicitation_id: &str,
        action: AskAction,
        content: Option<Map<String, Value>>,
    ) -> Result<()>;
    /// The session's events from now on. Ends after
    /// [`DriverEvent::Disconnected`].
    async fn events(&self, session: &str) -> Result<BoxStream<'static, DriverEvent>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_identify_by_run_and_step() {
        let labels = SessionLabels {
            graph: "g".into(),
            node: "n".into(),
            run: "r".into(),
            step: 2,
            revision: "h".into(),
        };
        assert_eq!(labels.all().len(), 5);
        assert_eq!(labels.all()[label::STEP], "2");
        assert_eq!(
            labels.identity().keys().collect::<Vec<_>>(),
            [label::RUN, label::STEP]
        );
    }

    #[test]
    fn harnesses_gate_a_host() {
        let host = HostInfo {
            id: "h1".into(),
            name: "local".into(),
            online: true,
            sandbox_provider: None,
            harnesses: BTreeMap::from([
                ("claude-sdk".into(), HarnessState::Ready),
                (
                    "codex".into(),
                    HarnessState::Unavailable("needs-auth".into()),
                ),
                ("pi".into(), HarnessState::Disabled),
            ]),
        };
        assert!(host.is("local") && host.is("h1"));
        host.check_harness("claude-sdk").unwrap();
        assert!(
            host.check_harness("codex")
                .unwrap_err()
                .contains("needs-auth")
        );
        assert!(host.check_harness("pi").unwrap_err().contains("turned off"));
        assert!(
            host.check_harness("x")
                .unwrap_err()
                .contains("isn't configured")
        );
        let offline = HostInfo {
            online: false,
            ..host
        };
        assert!(
            offline
                .check_harness("claude-sdk")
                .unwrap_err()
                .contains("offline")
        );
    }

    #[test]
    fn statuses_round_trip() {
        for s in [
            "launching",
            "running",
            "waiting",
            "idle",
            "failed",
            "paused",
        ] {
            assert_eq!(SessionStatus::parse(s).as_str(), s);
        }
    }

    #[test]
    fn errors_read_as_one_line() {
        let snap = SessionSnapshot {
            last_error: BTreeMap::from([
                ("code".into(), "llm".into()),
                ("message".into(), "rate limited".into()),
            ]),
            ..Default::default()
        };
        assert_eq!(snap.error_text(), "llm: rate limited");
        assert_eq!(SessionSnapshot::default().error_text(), "");
    }
}
