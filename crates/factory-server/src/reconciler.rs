//! The reconciler: it walks every executing graph, asks each live run's
//! executor what to do next, and does it through the `OmnigentDriver` and
//! the `CodeHost`.
//!
//! A [`Reconciler::pass`] does, for every graph:
//!
//! 1. **Amendments.** If the graph executes and its revision moved past the
//!    approved one, the amendment policies assess the changes. Accepted
//!    ones are approved on the spot, so a plan that changes while it runs
//!    keeps running; escalated ones raise one `approval` item, and the
//!    changed nodes wait (they're `Drifted`) while the rest goes on.
//! 2. **Dispatch.** Every `Ready` node with an executor and no run yet
//!    gets a queued run.
//! 3. **Runs.** Each live run is leased, observed (its sessions' streams,
//!    snapshots after a restart, detected PRs, the code host, review
//!    verdicts, attention) and advanced by its executor's actions.
//!    A plan's planning run follows the plan once it's approved: it reports
//!    the tasks' finished stages to its planner session (`plan.v1`, see
//!    `factory_core::executor::report`), so a plan root's observations
//!    carry the learnings and verdicts of its whole graph.
//! 4. **Resolutions.** Resolved attention items are acted on once: an ASK's
//!    answer goes to its session, a `retry` starts the next attempt, an
//!    accepted `approval` approves the current revision.
//!
//! Sessions are created with their run's and step's labels, the step index
//! fixed before the create and the record written after it. A crash between
//! the two leaves a session the next pass finds by its labels, so no step
//! ever gets a second session.
//!
//! Session streams are drained at the start of each pass, so a pass sees
//! exactly what arrived before it: tests drive [`Reconciler::pass`] by hand,
//! and [`Reconciler::spawn`] runs it on every graph change and every tick.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use buffa_types::google::protobuf::Timestamp;
use chrono::{DateTime, Utc};
use factory_core::codehost::CodeHost;
use factory_core::driver::{
    Ask, AskAction, DetectedPrs, DriverEvent, HostInfo, OmnigentDriver, SessionLabels,
    SessionRequest, SessionStatus, Target,
};
use factory_core::executor::{self, Action, ExecContext, Observations, SessionView, role};
use factory_core::{Error, Factory, Provenance, Result, artifact, payload, schema};
use factory_proto::workgraph::v1 as wg;
use futures::StreamExt;
use futures::stream::BoxStream;
use tokio::sync::Mutex;

use crate::runs::{self, Execution};

/// How the reconciler runs.
#[derive(Clone)]
pub struct Config {
    /// The time between passes when nothing happens.
    pub tick: Duration,
    /// How long a lease lasts; renewed every pass.
    pub lease: Duration,
    /// Poll the code host every this many passes.
    pub poll_every: u64,
    /// The lease holder's name: one per reconciler process.
    pub holder: String,
    /// The time; a test sets its own.
    pub clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tick: Duration::from_secs(5),
            lease: Duration::from_secs(30),
            poll_every: 6,
            holder: format!("reconciler-{}", factory_core::olai::new_ref()),
            clock: Arc::new(Utc::now),
        }
    }
}

/// A followed session: its stream, and what's been seen on it.
#[derive(Default)]
struct Followed {
    stream: Option<BoxStream<'static, DriverEvent>>,
    view: SessionView,
    prs: DetectedPrs,
    asks: Vec<Ask>,
    /// Re-read the snapshot (after the stream went idle: the cost, the
    /// error, the turn count).
    stale: bool,
}

/// See the [module docs](self).
pub struct Reconciler {
    factory: Arc<Factory>,
    driver: Arc<dyn OmnigentDriver>,
    code_host: Option<Arc<dyn CodeHost>>,
    config: Config,
    sessions: Mutex<HashMap<String, Followed>>,
    passes: std::sync::atomic::AtomicU64,
    who: Provenance,
}

impl Reconciler {
    pub fn new(
        factory: Arc<Factory>,
        driver: Arc<dyn OmnigentDriver>,
        code_host: Option<Arc<dyn CodeHost>>,
        config: Config,
    ) -> Self {
        let who = Provenance::new(config.holder.clone(), "reconciler");
        Self {
            factory,
            driver,
            code_host,
            config,
            sessions: Mutex::new(HashMap::new()),
            passes: std::sync::atomic::AtomicU64::new(0),
            who,
        }
    }

    fn now(&self) -> DateTime<Utc> {
        (self.config.clock)()
    }

    /// Run a pass on every graph change and every tick, until the factory
    /// is dropped.
    pub fn spawn(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut changes = self.factory.subscribe();
            loop {
                if let Err(e) = self.pass().await {
                    tracing::warn!("reconciler pass: {e}");
                }
                tokio::select! {
                    () = tokio::time::sleep(self.config.tick) => {}
                    change = changes.recv() => {
                        if matches!(change, Err(tokio::sync::broadcast::error::RecvError::Closed)) {
                            return;
                        }
                        // Let a burst of writes settle into one pass.
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        while changes.try_recv().is_ok() {}
                    }
                }
            }
        })
    }

    /// One pass over every graph.
    pub async fn pass(&self) -> Result<()> {
        let pass = self
            .passes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        for summary in self.factory.list_graphs().await? {
            let root = summary.root.r#ref;
            if let Err(e) = self.graph(&root, pass).await {
                tracing::warn!(root, "reconciling: {e}");
            }
        }
        Ok(())
    }

    async fn graph(&self, root: &str, pass: u64) -> Result<()> {
        let execution = runs::execution(&self.factory, root).await?;
        if execution.is_some() {
            self.amend(root).await?;
            self.dispatch(root).await?;
        }
        let choice = execution
            .as_ref()
            .map(Execution::choice)
            .unwrap_or_default();
        for run in self.factory.runs(root).await? {
            if runs::is_live(&run) {
                if let Err(e) = self.drive(root, run.clone(), &choice, pass).await {
                    tracing::warn!(root, run = %run.r#ref, "driving: {e}");
                }
            } else if run.state.as_known() == Some(wg::RunState::RUN_STATE_CANCELED) {
                self.interrupt_canceled(run).await?;
            }
        }
        self.apply_resolutions(root).await
    }

    // --- amendments and dispatch ----------------------------------------

    async fn amend(&self, root: &str) -> Result<()> {
        let state = self.factory.graph(root).await?;
        let Some(approved) = state.approved.clone() else {
            return Ok(());
        };
        if approved == state.revision {
            return Ok(());
        }
        let changes = self
            .factory
            .diff(root, Some(&approved), Some(&state.revision), true)
            .await?;
        let all_runs = self.factory.runs(root).await?;
        let revisions = self.factory.revisions(root).await?;
        let since: Vec<Provenance> = revisions
            .iter()
            .rev()
            .skip_while(|r| r.root_hash != approved)
            .skip(1)
            .map(|r| r.provenance.clone())
            .collect();
        let outcome = factory_core::amendment::assess(
            self.factory.extensions().amendment_policies(),
            &changes,
            &factory_core::amendment::AmendmentContext {
                graph: &state.graph,
                runs: &all_runs,
                registry: self.factory.registry(),
                provenance: &since,
            },
        );
        if outcome.all_accepted() {
            let amended = Provenance::new(self.who.actor.clone(), runs::AMENDMENT);
            match self.factory.approve(root, &state.revision, &amended).await {
                Ok(_) => {
                    self.factory
                        .append_event(
                            root,
                            root,
                            runs::AMENDMENT,
                            format!(
                                "accepted {} change(s): {}",
                                outcome.accepted.len(),
                                outcome.accepted.join(", ")
                            ),
                            &amended,
                        )
                        .await?;
                    tracing::info!(root, revision = %state.revision, "amendment accepted");
                }
                // The plan changed again in between: the next pass sees it.
                Err(Error::FailedPrecondition(_)) => {}
                Err(e) => return Err(e),
            }
            return Ok(());
        }
        let open = self.factory.attention(Some(root), false).await?;
        let asked = open.iter().any(|a| {
            a.node == root
                && a.run.is_empty()
                && a.kind.as_known() == Some(wg::AttentionKind::ATTENTION_KIND_APPROVAL)
        });
        if !asked {
            let reasons: Vec<String> = outcome
                .escalated
                .iter()
                .map(|(_, _, why)| why.clone())
                .collect();
            self.factory
                .raise_attention(
                    wg::Attention {
                        kind: wg::AttentionKind::ATTENTION_KIND_APPROVAL.into(),
                        node: root.into(),
                        summary: format!(
                            "Changes to the plan need a review: {}",
                            reasons.join("; ")
                        ),
                        ..Default::default()
                    },
                    &self.who,
                )
                .await?;
        }
        Ok(())
    }

    async fn dispatch(&self, root: &str) -> Result<()> {
        let state = self.factory.graph(root).await?;
        let all_runs = self.factory.runs(root).await?;
        for node in &state.graph.nodes {
            let ready = node.status.as_option().is_some_and(|s| {
                s.conditions
                    .iter()
                    .any(|c| c.r#type == factory_core::condition::READY && c.status)
            });
            if !ready || all_runs.iter().any(|r| r.node == node.r#ref) {
                continue;
            }
            let Some(executor) = self.factory.extensions().executor(&node.kind) else {
                continue;
            };
            let run = runs::queue_run(
                &self.factory,
                root,
                &node.r#ref,
                executor.name(),
                "",
                "",
                &self.who,
            )
            .await?;
            tracing::info!(root, node = %node.r#ref, run = %run.r#ref, "dispatched");
        }
        Ok(())
    }

    // --- one run --------------------------------------------------------

    async fn drive(
        &self,
        root: &str,
        run: wg::Run,
        choice: &wg::PlacementChoice,
        pass: u64,
    ) -> Result<()> {
        let until = self.now()
            + chrono::Duration::from_std(self.config.lease)
                .unwrap_or(chrono::Duration::seconds(30));
        let Some(mut run) = self
            .factory
            .lease_run(root, &run.r#ref, &self.config.holder, until)
            .await?
        else {
            return Ok(());
        };
        let before = run.clone();
        let state = self.factory.graph(root).await?;
        let node = state
            .graph
            .nodes
            .iter()
            .find(|n| n.r#ref == run.node)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("node {}", run.node)))?;
        let Some(exec) = self.factory.extensions().executor(&node.kind) else {
            runs::finish(
                &mut run,
                wg::RunState::RUN_STATE_FAILED,
                "no executor for its node kind",
            );
            return self.factory.put_run(&run, &self.who).await;
        };

        let seen = self.observe(root, &node, &mut run, pass).await?;
        let all_runs = self.factory.runs(root).await?;
        let actions = exec.next(&ExecContext {
            graph: &state.graph,
            runs: &all_runs,
            node: &node,
            run: &run,
            seen: &seen,
            extensions: self.factory.extensions(),
            registry: self.factory.registry(),
        });
        let mut retry = None;
        for action in actions {
            if let Err(e) = self
                .apply(root, &node, &mut run, choice, &seen, action, &mut retry)
                .await
            {
                // A transient failure (Omnigent unreachable, a crash) leaves
                // the run as it was recorded: the next pass repeats the
                // action, and create-or-find finds a session created in
                // between.
                if matches!(e, Error::Internal(_)) {
                    return Err(e);
                }
                // A dispatch the registry or Omnigent refuses won't fix
                // itself: fail the run and tell a person.
                tracing::warn!(run = %run.r#ref, "applying: {e}");
                runs::finish(&mut run, wg::RunState::RUN_STATE_FAILED, &e.to_string());
                self.raise(
                    root,
                    &run,
                    wg::AttentionKind::ATTENTION_KIND_STALLED,
                    format!("can't continue: {e}"),
                    None,
                )
                .await?;
                break;
            }
        }
        if runs::is_live(&run) && !run.steps.is_empty() {
            let waiting = seen
                .sessions
                .values()
                .any(|v| v.status == SessionStatus::Waiting);
            run.state = if waiting {
                wg::RunState::RUN_STATE_INPUT_REQUIRED
            } else {
                wg::RunState::RUN_STATE_WORKING
            }
            .into();
        }
        run.cost_usd = run
            .steps
            .iter()
            .filter_map(executor::session_of)
            .map(|s| s.cost_usd)
            .sum();
        if changed(&before, &run) {
            run.updated_at = Timestamp::now().into();
            self.factory.put_run(&run, &self.who).await?;
        }
        if let Some(reason) = retry {
            runs::queue_run(
                &self.factory,
                root,
                &run.node,
                &run.executor,
                &run.profile,
                &run.environment,
                &self.who,
            )
            .await?;
            tracing::info!(run = %run.r#ref, "retrying: {reason}");
        }
        Ok(())
    }

    /// Observe the run: its active sessions, their PRs, the code host, the
    /// node's verdicts and attention. Folds what it sees into the run.
    async fn observe(
        &self,
        root: &str,
        node: &wg::Node,
        run: &mut wg::Run,
        pass: u64,
    ) -> Result<Observations> {
        let mut seen = Observations::at(self.now());
        let active: Vec<(u32, String)> = run
            .steps
            .iter()
            .filter(|s| s.state.as_known() == Some(wg::StepState::STEP_STATE_ACTIVE))
            .filter_map(|s| executor::session_of(s).map(|o| (s.index, o.session_id)))
            .filter(|(_, id)| !id.is_empty())
            .collect();
        for (index, session) in active {
            let (view, prs, asks) = self.follow(&session).await;
            if let Some(step) = run.steps.iter_mut().find(|s| s.index == index)
                && let Some(mut payload) = executor::session_of(step)
            {
                payload.status = if view.missing {
                    "missing".into()
                } else {
                    view.status.as_str().into()
                };
                view.error.clone_into(&mut payload.last_error);
                if view.cost_usd > 0.0 {
                    payload.cost_usd = view.cost_usd;
                }
                step.payload = payload::pack(&payload).into();
            }
            let detected: Vec<wg::PullRequest> = prs
                .prs
                .iter()
                .map(|p| wg::PullRequest {
                    url: p.url.clone(),
                    repo: p.repo.clone(),
                    number: p.number,
                    title: p.title.clone(),
                    state: p.state.clone(),
                    draft: p.draft,
                    ..Default::default()
                })
                .collect();
            artifact::merge_prs(
                run,
                &detected,
                wg::ArtifactSource::ARTIFACT_SOURCE_OMNIGENT,
                index,
            );
            for ask in asks {
                self.raise_ask(root, run, index, &session, &ask).await?;
            }
            seen.sessions.insert(index, view);
        }
        if let Some(host) = &self.code_host
            && pass.is_multiple_of(self.config.poll_every.max(1))
        {
            let open: Vec<String> = executor::pull_requests(run)
                .iter()
                .filter(|p| p.state != "merged" && p.state != "closed" && host.handles(&p.url))
                .map(|p| p.url.clone())
                .collect();
            for url in open {
                match host.pull_request(&url).await {
                    Ok(status) => artifact::merge_prs(
                        run,
                        &[artifact::from_status(&status)],
                        wg::ArtifactSource::ARTIFACT_SOURCE_CODE_HOST,
                        0,
                    ),
                    Err(e) => tracing::warn!(url, "code host: {e}"),
                }
            }
        }
        self.group(node, run);
        let annotations = self.factory.annotations(&node.r#ref).await?;
        seen.verdicts = annotations
            .iter()
            .filter(|a| a.kind == wg::ReviewVerdict::TYPE_URL)
            .filter_map(|a| {
                a.payload
                    .as_option()
                    .and_then(|p| p.unpack_unchecked().ok())
            })
            .collect();
        if node.kind == schema::TASK {
            seen.annotations = annotations
                .into_iter()
                .filter(|a| a.kind == wg::Learning::TYPE_URL)
                .collect();
        }
        seen.attention = self
            .factory
            .attention(Some(root), true)
            .await?
            .into_iter()
            .filter(|a| a.node == node.r#ref)
            .collect();
        if node.kind == schema::PLAN && run.stage == executor::stage::FOLLOWING {
            for n in &self.factory.graph(root).await?.graph.nodes {
                seen.annotations.extend(
                    self.factory
                        .annotations(&n.r#ref)
                        .await?
                        .into_iter()
                        .filter(|a| {
                            a.kind == wg::Learning::TYPE_URL
                                || a.kind == wg::ReviewVerdict::TYPE_URL
                        }),
                );
            }
        }
        Ok(seen)
    }

    /// Let the task's hooks group and order its PRs.
    fn group(&self, node: &wg::Node, run: &mut wg::Run) {
        if node.kind != schema::TASK {
            return;
        }
        let task: wg::Task = executor::payload(node);
        let hooks: Vec<_> = task
            .extensions
            .iter()
            .filter_map(|e| {
                self.factory
                    .extensions()
                    .task_hook(&e.type_url)
                    .map(|h| (e, h))
            })
            .collect();
        if hooks.is_empty() {
            return;
        }
        let mut prs: Vec<wg::PullRequest> =
            executor::pull_requests(run).into_iter().cloned().collect();
        for (e, h) in hooks {
            h.group(e, &mut prs);
        }
        let mut others: Vec<wg::Artifact> = Vec::new();
        let mut by_url: BTreeMap<String, wg::Artifact> = BTreeMap::new();
        for a in run.artifacts.drain(..) {
            match &a.artifact {
                Some(wg::artifact::Artifact::PullRequest(p)) => {
                    by_url.insert(p.url.clone(), a);
                }
                _ => others.push(a),
            }
        }
        for pr in prs {
            if let Some(mut a) = by_url.remove(&pr.url) {
                a.artifact = Some(wg::artifact::Artifact::PullRequest(Box::new(pr)));
                run.artifacts.push(a);
            }
        }
        run.artifacts.extend(by_url.into_values());
        run.artifacts.extend(others);
    }

    /// Follow a session: open its stream (and snapshot it) if needed, then
    /// drain what arrived. Returns its view, PRs and new asks.
    async fn follow(&self, session: &str) -> (SessionView, DetectedPrs, Vec<Ask>) {
        let now = self.now();
        let mut sessions = self.sessions.lock().await;
        let f = sessions.entry(session.to_owned()).or_default();
        if f.stream.is_none() {
            // The stream first, so nothing falls between it and the snapshot.
            match self.driver.events(session).await {
                Ok(stream) => f.stream = Some(stream),
                Err(Error::NotFound(_)) => {
                    f.view = SessionView {
                        missing: true,
                        ..Default::default()
                    };
                    return (f.view.clone(), f.prs.clone(), Vec::new());
                }
                Err(e) => tracing::warn!(session, "stream: {e}"),
            }
            f.stale = true;
        }
        if let Some(stream) = &mut f.stream {
            while let Some(next) = futures::FutureExt::now_or_never(stream.next()) {
                let Some(event) = next else {
                    f.stream = None;
                    break;
                };
                match event {
                    DriverEvent::TurnStarted => {
                        f.view.turns += 1;
                        f.view.idle_since = None;
                    }
                    DriverEvent::Status(status) => {
                        if status == SessionStatus::Idle {
                            f.view.idle_since.get_or_insert(now);
                            f.stale = true;
                        }
                        f.view.status = status;
                    }
                    DriverEvent::TurnFailed(e) => f.view.error = e,
                    DriverEvent::Ask(ask) => f.asks.push(ask),
                    DriverEvent::PullRequests(prs) => {
                        f.view.prs_synced = prs.synced;
                        f.prs = prs;
                    }
                    DriverEvent::Disconnected => {
                        f.stream = None;
                        break;
                    }
                }
            }
        }
        if f.stale {
            f.stale = false;
            match self.driver.snapshot(session).await {
                Ok(snap) => {
                    let idle = snap.status == SessionStatus::Idle;
                    f.view.error = snap.error_text();
                    f.view.status = snap.status;
                    f.view.turns = f.view.turns.max(snap.turns);
                    f.view.cost_usd = snap.cost_usd;
                    f.view.missing = false;
                    if idle {
                        f.view.idle_since.get_or_insert(now);
                    }
                    for ask in snap.asks {
                        if !f
                            .asks
                            .iter()
                            .any(|a| a.elicitation_id == ask.elicitation_id)
                        {
                            f.asks.push(ask);
                        }
                    }
                }
                Err(Error::NotFound(_)) => f.view.missing = true,
                Err(e) => tracing::warn!(session, "snapshot: {e}"),
            }
            match self.driver.pull_requests(session).await {
                Ok(prs) => {
                    f.view.prs_synced = prs.synced;
                    f.prs = prs;
                }
                Err(e) => tracing::debug!(session, "pull requests: {e}"),
            }
        }
        (f.view.clone(), f.prs.clone(), std::mem::take(&mut f.asks))
    }

    async fn raise_ask(
        &self,
        root: &str,
        run: &wg::Run,
        step: u32,
        session: &str,
        ask: &Ask,
    ) -> Result<()> {
        let known = self
            .factory
            .attention(Some(root), true)
            .await?
            .into_iter()
            .any(|a| {
                a.kind.as_known() == Some(wg::AttentionKind::ATTENTION_KIND_ASK)
                    && a.detail
                        .as_option()
                        .and_then(|d| d.unpack_unchecked::<wg::AskDetail>().ok())
                        .is_some_and(|d| d.elicitation_id == ask.elicitation_id)
            });
        if known {
            return Ok(());
        }
        self.factory
            .raise_attention(
                wg::Attention {
                    kind: wg::AttentionKind::ATTENTION_KIND_ASK.into(),
                    node: run.node.clone(),
                    run: run.r#ref.clone(),
                    step,
                    summary: ask.message.clone(),
                    detail: payload::pack(&wg::AskDetail {
                        session_id: session.into(),
                        elicitation_id: ask.elicitation_id.clone(),
                        message: ask.message.clone(),
                        tool: ask.tool.clone(),
                        ..Default::default()
                    })
                    .into(),
                    ..Default::default()
                },
                &self.who,
            )
            .await?;
        Ok(())
    }

    async fn raise(
        &self,
        root: &str,
        run: &wg::Run,
        kind: wg::AttentionKind,
        summary: String,
        step: Option<u32>,
    ) -> Result<()> {
        let open = self.factory.attention(Some(root), false).await?;
        if open
            .iter()
            .any(|a| a.run == run.r#ref && a.kind.as_known() == Some(kind))
        {
            return Ok(());
        }
        self.factory
            .raise_attention(
                wg::Attention {
                    kind: kind.into(),
                    node: run.node.clone(),
                    run: run.r#ref.clone(),
                    step: step.unwrap_or_default(),
                    summary,
                    ..Default::default()
                },
                &self.who,
            )
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply(
        &self,
        root: &str,
        node: &wg::Node,
        run: &mut wg::Run,
        choice: &wg::PlacementChoice,
        seen: &Observations,
        action: Action,
        retry: &mut Option<String>,
    ) -> Result<()> {
        match action {
            Action::StartSession {
                step,
                role,
                profile,
                prompt,
            } => {
                self.start_session(root, node, run, choice, &step, &role, &profile, prompt)
                    .await?;
            }
            Action::SendMessage { step, text } => {
                let s = run
                    .steps
                    .iter_mut()
                    .find(|s| s.index == step)
                    .ok_or_else(|| Error::internal(format!("no step {step}")))?;
                let mut session = executor::session_of(s)
                    .ok_or_else(|| Error::internal(format!("step {step} has no session")))?;
                self.driver.send(&session.session_id, &text).await?;
                session.follow_ups += 1;
                s.payload = payload::pack(&session).into();
            }
            Action::SetStep { step, state } => {
                if let Some(s) = run.steps.iter_mut().find(|s| s.index == step) {
                    s.state = state.into();
                    s.ended_at = if state == wg::StepState::STEP_STATE_ACTIVE {
                        Default::default()
                    } else {
                        Timestamp::now().into()
                    };
                }
            }
            Action::Wait { observed } => {
                let wait = wg::GateWait {
                    gate: node.r#ref.clone(),
                    observed,
                    checked_at: Timestamp::from_unix_secs(seen.now.timestamp()).into(),
                    ..Default::default()
                };
                let packed = payload::pack(&wait);
                match run
                    .steps
                    .iter_mut()
                    .find(|s| s.name == executor::step::GATE)
                {
                    Some(s) => {
                        let old = s
                            .payload
                            .as_option()
                            .and_then(|p| p.unpack_unchecked::<wg::GateWait>().ok());
                        // Re-checking isn't news; a new observation is.
                        if old.is_none_or(|o| o.observed != wait.observed) {
                            s.payload = packed.into();
                        }
                    }
                    None => {
                        let index = u32::try_from(run.steps.len()).unwrap_or(u32::MAX);
                        run.steps.push(wg::Step {
                            index,
                            kind: wg::StepKind::STEP_KIND_WAIT.into(),
                            state: wg::StepState::STEP_STATE_ACTIVE.into(),
                            name: executor::step::GATE.into(),
                            payload: packed.into(),
                            started_at: Timestamp::now().into(),
                            ..Default::default()
                        });
                    }
                }
            }
            Action::SetStage(stage) => run.stage = stage,
            Action::SetState(state) => {
                // Recorded before what follows is applied: the state may say
                // what's about to be sent.
                run.executor_state = state.into();
                run.updated_at = Timestamp::now().into();
                self.factory.put_run(run, &self.who).await?;
            }
            Action::RaiseAttention {
                kind,
                summary,
                step,
            } => {
                self.raise(root, run, kind, summary, step).await?;
            }
            Action::Complete { node_done } => {
                runs::finish(run, wg::RunState::RUN_STATE_COMPLETED, "");
                if node_done {
                    self.factory
                        .set_phase(&node.r#ref, wg::Phase::PHASE_DONE, &self.who)
                        .await?;
                }
            }
            Action::Fail { reason } => {
                self.interrupt_active(run).await;
                runs::finish(run, wg::RunState::RUN_STATE_FAILED, &reason);
            }
            Action::Retry { reason } => {
                self.interrupt_active(run).await;
                runs::finish(run, wg::RunState::RUN_STATE_FAILED, &reason);
                *retry = Some(reason);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_session(
        &self,
        root: &str,
        node: &wg::Node,
        run: &mut wg::Run,
        choice: &wg::PlacementChoice,
        step_name: &str,
        role_name: &str,
        profile_name: &str,
        prompt: String,
    ) -> Result<()> {
        let index = u32::try_from(run.steps.len()).unwrap_or(u32::MAX);
        let registry = self.factory.registry();
        let profile_name = if role_name == role::EXECUTOR && !run.profile.is_empty() {
            run.profile.clone()
        } else {
            profile_name.to_owned()
        };
        let profile = registry.profile(&profile_name).cloned().ok_or_else(|| {
            Error::precondition(format!("profile {profile_name:?} isn't in the registry"))
        })?;
        let run_env = (!run.environment.is_empty())
            .then_some((node.r#ref.as_str(), run.environment.as_str()));
        let placements = self.factory.placements(root, choice, run_env).await?;
        let placement = placements
            .iter()
            .find(|p| p.node == node.r#ref && p.role == role_name)
            .ok_or_else(|| {
                Error::precondition(format!("nothing places the {role_name} of {}", node.r#ref))
            })?;
        if !placement.problem.is_empty() {
            return Err(Error::precondition(placement.problem.clone()));
        }
        // `decide` leaves the pick to the Decider, which isn't built yet: its
        // deterministic fallback is the first eligible environment.
        let env_name = if placement.environment.is_empty() {
            placement.eligible.first().cloned().unwrap_or_default()
        } else {
            placement.environment.clone()
        };
        let env = registry.environment(&env_name).cloned().ok_or_else(|| {
            Error::precondition(format!("no environment can run the {role_name}"))
        })?;
        let agent = self.driver.resolve_agent(&profile.agent).await?;
        let harness = (!profile.harness.is_empty())
            .then(|| profile.harness.clone())
            .or_else(|| agent.harness.clone());
        let task: wg::Task = executor::payload(node);
        let repo_name = task.repo.rsplit('/').next().unwrap_or_default().to_owned();
        let base = if !task.base_branch.is_empty() {
            task.base_branch.clone()
        } else {
            registry
                .repo(&task.repo)
                .map(|r| r.default_branch.clone())
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| "main".into())
        };
        let branch = (role_name == role::EXECUTOR && !task.repo.is_empty())
            .then(|| format!("factory/{}-{}", short(&run.r#ref), run.attempt));
        let (target, host_type, host_id, provider, sandbox_profile, workspace) =
            match env.placement.as_option().and_then(|p| p.target.as_ref()) {
                Some(wg::placement::Target::Host(h)) => {
                    let hosts = self.driver.hosts().await?;
                    let host: HostInfo =
                        hosts.into_iter().find(|x| x.is(&h.host)).ok_or_else(|| {
                            Error::precondition(format!("Omnigent has no host {}", h.host))
                        })?;
                    if let Some(harness) = &harness {
                        host.check_harness(harness).map_err(Error::precondition)?;
                    }
                    let workspace = if repo_name.is_empty() {
                        h.workspace_root.clone()
                    } else {
                        format!("{}/{repo_name}", h.workspace_root.trim_end_matches('/'))
                    };
                    (
                        Target::Host {
                            host_id: host.id.clone(),
                            workspace: workspace.clone(),
                        },
                        "external",
                        host.id,
                        String::new(),
                        String::new(),
                        workspace,
                    )
                }
                Some(wg::placement::Target::Managed(m)) => {
                    let workspace = if task.repo.is_empty() {
                        String::new()
                    } else {
                        format!("https://github.com/{}.git#{base}", task.repo)
                    };
                    (
                        Target::Managed {
                            provider: m.provider.clone(),
                            sandbox_profile: m.sandbox_profile.clone(),
                            workspace: workspace.clone(),
                        },
                        "managed",
                        String::new(),
                        m.provider.clone(),
                        m.sandbox_profile.clone(),
                        workspace,
                    )
                }
                None => {
                    return Err(Error::precondition(format!(
                        "environment {} has no placement",
                        env.name
                    )));
                }
            };
        let labels = SessionLabels {
            graph: root.into(),
            node: node.r#ref.clone(),
            run: run.r#ref.clone(),
            step: index,
            revision: run.revision.clone(),
        };
        let request = SessionRequest {
            agent_id: agent.id.clone(),
            title: format!("{} · {step_name}", node.spec.title),
            labels: labels.clone(),
            target,
            branch: branch.clone(),
            base_branch: branch.as_ref().map(|_| base.clone()),
            harness: (!profile.harness.is_empty()).then(|| profile.harness.clone()),
            model: (!profile.model.is_empty()).then(|| profile.model.clone()),
            reasoning_effort: (!profile.reasoning_effort.is_empty())
                .then(|| profile.reasoning_effort.clone()),
            mcp_registry_services: profile.mcp_registry_services.clone(),
            prompt,
        };
        let created = self.driver.create_or_find(&request).await?;
        let session_id = created.session.id.clone();
        let spec = wg::ResolvedSessionSpec {
            profile: profile.name.clone(),
            agent_id: agent.id,
            agent_name: agent.name,
            agent_version: agent.version,
            harness: harness.unwrap_or_default(),
            model: profile.model.clone(),
            reasoning_effort: profile.reasoning_effort.clone(),
            host_type: host_type.into(),
            host_id,
            sandbox_provider: provider,
            sandbox_profile,
            workspace,
            branch: branch.unwrap_or_default(),
            base_branch: base,
            labels: labels.all().into_iter().collect(),
            environment: env.name.clone(),
            placement_source: placement.source,
            mcp_registry_services: profile.mcp_registry_services.clone(),
            ..Default::default()
        };
        run.steps.push(wg::Step {
            index,
            kind: wg::StepKind::STEP_KIND_SESSION.into(),
            state: wg::StepState::STEP_STATE_ACTIVE.into(),
            name: step_name.into(),
            payload: payload::pack(&wg::OmnigentSession {
                session_id: session_id.clone(),
                spec: spec.into(),
                status: created.session.status.as_str().into(),
                ..Default::default()
            })
            .into(),
            started_at: Timestamp::now().into(),
            ..Default::default()
        });
        run.session_id = session_id;
        if run.state.as_known() == Some(wg::RunState::RUN_STATE_QUEUED) {
            run.state = wg::RunState::RUN_STATE_WORKING.into();
        }
        tracing::info!(run = %run.r#ref, step = index, session = %run.session_id, created = created.created, "session");
        Ok(())
    }

    async fn interrupt_active(&self, run: &wg::Run) {
        for step in &run.steps {
            if step.state.as_known() != Some(wg::StepState::STEP_STATE_ACTIVE) {
                continue;
            }
            if let Some(s) = executor::session_of(step)
                && s.status != "failed"
                && s.status != "missing"
                && let Err(e) = self.driver.interrupt(&s.session_id).await
            {
                tracing::debug!(session = %s.session_id, "interrupt: {e}");
            }
        }
    }

    /// A canceled run still has active steps: interrupt their sessions and
    /// close them.
    async fn interrupt_canceled(&self, mut run: wg::Run) -> Result<()> {
        let active = run
            .steps
            .iter()
            .any(|s| s.state.as_known() == Some(wg::StepState::STEP_STATE_ACTIVE));
        if !active {
            return Ok(());
        }
        self.interrupt_active(&run).await;
        runs::finish(&mut run, wg::RunState::RUN_STATE_CANCELED, "");
        self.factory.put_run(&run, &self.who).await
    }

    // --- resolutions ----------------------------------------------------

    async fn apply_resolutions(&self, root: &str) -> Result<()> {
        for mut item in self.factory.attention(Some(root), true).await? {
            if !item.resolved || item.applied {
                continue;
            }
            let kind = item.kind.as_known();
            match (kind, item.resolution.as_str()) {
                (Some(wg::AttentionKind::ATTENTION_KIND_ASK), resolution) => {
                    let detail = item
                        .detail
                        .as_option()
                        .and_then(|d| d.unpack_unchecked::<wg::AskDetail>().ok())
                        .unwrap_or_default();
                    let action = match resolution {
                        "accept" => AskAction::Accept,
                        "decline" => AskAction::Decline,
                        _ => AskAction::Cancel,
                    };
                    if let Err(e) = self
                        .driver
                        .resolve_ask(&detail.session_id, &detail.elicitation_id, action, None)
                        .await
                    {
                        tracing::warn!(item = %item.r#ref, "forwarding the answer: {e}");
                        continue;
                    }
                }
                (_, "retry") if !item.run.is_empty() => {
                    let run = self.factory.run(root, &item.run).await?;
                    runs::retry(&self.factory, root, &run, "retried by a person", &self.who)
                        .await?;
                }
                (Some(wg::AttentionKind::ATTENTION_KIND_APPROVAL), "accept") => {
                    let state = self.factory.graph(root).await?;
                    if state.approved.as_deref() != Some(state.revision.as_str()) {
                        let by = Provenance::new(item.resolved_by.clone(), "attention");
                        if let Err(e) = self.factory.approve(root, &state.revision, &by).await {
                            tracing::warn!(root, "approving: {e}");
                            continue;
                        }
                    }
                }
                _ => {}
            }
            item.applied = true;
            self.factory
                .update_attention(root, &item, &self.who)
                .await?;
        }
        Ok(())
    }
}

/// Whether a run changed in anything but its lease.
fn changed(a: &wg::Run, b: &wg::Run) -> bool {
    let strip = |r: &wg::Run| wg::Run {
        lease_expiry: Default::default(),
        lease_holder: String::new(),
        updated_at: Default::default(),
        ..r.clone()
    };
    strip(a) != strip(b)
}

/// The last eight characters of a ref, for branch names.
fn short(r: &str) -> &str {
    let r = r.rsplit(':').next().unwrap_or(r);
    &r[r.len().saturating_sub(8)..]
}
