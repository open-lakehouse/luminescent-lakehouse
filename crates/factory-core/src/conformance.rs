//! One behavioral suite for every backend: run [`run_all`] with a factory
//! over a fresh backend, and it panics on the first check that fails.
//!
//! It covers schema validation, the hierarchy and moves, optimistic updates,
//! derived conditions and the ready set, revisions and their diffs, deletes,
//! annotations, runs and change notifications.

use std::collections::HashMap;
use std::future::Future;

use factory_proto::workgraph::v1 as wg;
use plan_tree::Change;

use crate::payload::{self, task};
use crate::schema::{BLOCKED_BY, CHILD_OF, DECISION, PLAN, RELATES_TO, TASK};
use crate::{Error, Factory, Provenance, condition};

fn who() -> Provenance {
    Provenance::new("tester", "conformance")
}

fn edge(kind: &str, from: &str, to: &str) -> wg::Edge {
    wg::Edge {
        kind: kind.into(),
        from: from.into(),
        to: to.into(),
        ..Default::default()
    }
}

async fn plan(f: &Factory, title: &str) -> String {
    f.create_node(
        None,
        PLAN,
        payload::spec(title, "", &wg::Plan::default()),
        HashMap::new(),
        &who(),
    )
    .await
    .unwrap()
    .r#ref
}

async fn item(f: &Factory, parent: &str, title: &str) -> String {
    f.create_node(
        Some(parent),
        TASK,
        payload::spec(title, "", &task("works", &["src/lib.rs"])),
        HashMap::new(),
        &who(),
    )
    .await
    .unwrap()
    .r#ref
}

fn condition<'a>(g: &'a wg::Graph, node: &str, kind: &str) -> Option<&'a wg::Condition> {
    g.nodes
        .iter()
        .find(|n| n.r#ref == node)?
        .status
        .as_option()?
        .conditions
        .iter()
        .find(|c| c.r#type == kind)
}

fn ready(g: &wg::Graph, node: &str) -> bool {
    condition(g, node, condition::READY).is_some_and(|c| c.status)
}

/// Run every check, each against a fresh factory from `make`.
pub async fn run_all<F, Fut>(make: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = Factory>,
{
    creates_and_reads(&make().await).await;
    rejects_what_the_schema_forbids(&make().await).await;
    moves_within_the_hierarchy(&make().await).await;
    refuses_stale_etags(&make().await).await;
    derives_the_ready_set(&make().await).await;
    records_revisions_for_content_only(&make().await).await;
    deletes(&make().await).await;
    keeps_annotations_and_runs(&make().await).await;
    keeps_run_steps_and_artifacts(&make().await).await;
    gates_block_like_items(&make().await).await;
    notifies_watchers(&make().await).await;
    keeps_attention(&make().await).await;
    leases_runs(&make().await).await;
}

async fn keeps_attention(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let mut rx = f.subscribe();
    let raised = f
        .raise_attention(
            wg::Attention {
                kind: wg::AttentionKind::ATTENTION_KIND_ASK.into(),
                node: a.clone(),
                summary: "may I?".into(),
                ..Default::default()
            },
            &p,
        )
        .await
        .unwrap();
    assert!(!raised.r#ref.is_empty() && raised.created_at.as_option().is_some());
    let change = rx.recv().await.unwrap();
    assert_eq!(
        change.attention.as_ref().map(|i| &i.r#ref),
        Some(&raised.r#ref)
    );
    let second = f
        .raise_attention(
            wg::Attention {
                kind: wg::AttentionKind::ATTENTION_KIND_GATE.into(),
                node: a.clone(),
                ..Default::default()
            },
            &p,
        )
        .await
        .unwrap();
    let open = f.attention(Some(&root), false).await.unwrap();
    assert_eq!(
        open.iter().map(|i| &i.r#ref).collect::<Vec<_>>(),
        [&raised.r#ref, &second.r#ref],
        "oldest first"
    );
    let resolved = f
        .resolve_attention(&raised.r#ref, "accept", "go ahead", &p)
        .await
        .unwrap();
    assert!(resolved.resolved && resolved.resolution == "accept");
    assert_eq!(resolved.resolved_by, "tester");
    assert!(matches!(
        f.resolve_attention(&raised.r#ref, "accept", "", &p).await,
        Err(Error::FailedPrecondition(_))
    ));
    assert_eq!(f.attention(Some(&root), false).await.unwrap().len(), 1);
    assert_eq!(f.attention(None, true).await.unwrap().len(), 2);
}

async fn leases_runs(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let run = wg::Run {
        r#ref: crate::olai::new_ref(),
        node: a.clone(),
        attempt: 1,
        state: wg::RunState::RUN_STATE_QUEUED.into(),
        ..Default::default()
    };
    f.put_run(&run, &p).await.unwrap();
    assert_eq!(f.find_run(&run.r#ref).await.unwrap().0, root);
    let soon = chrono::Utc::now() + chrono::Duration::seconds(60);
    let held = f
        .lease_run(&root, &run.r#ref, "r1", soon)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.lease_holder, "r1");
    assert!(
        f.lease_run(&root, &run.r#ref, "r2", soon)
            .await
            .unwrap()
            .is_none(),
        "another holder's live lease"
    );
    assert!(
        f.lease_run(&root, &run.r#ref, "r1", soon)
            .await
            .unwrap()
            .is_some(),
        "renews"
    );
    let past = chrono::Utc::now() - chrono::Duration::seconds(5);
    f.lease_run(&root, &run.r#ref, "r1", past).await.unwrap();
    let taken = f
        .lease_run(&root, &run.r#ref, "r2", soon)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(taken.lease_holder, "r2", "an expired lease is taken over");
}

async fn creates_and_reads(f: &Factory) {
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let b = item(f, &a, "B").await;

    let g = f.graph(&root).await.unwrap().graph;
    assert_eq!(g.root, root);
    assert_eq!(g.nodes.len(), 3);
    let parents: Vec<(&str, &str)> = g
        .edges
        .iter()
        .filter(|e| e.kind == CHILD_OF)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    assert!(parents.contains(&(a.as_str(), root.as_str())));
    assert!(parents.contains(&(b.as_str(), a.as_str())));

    let node = f.node(&b).await.unwrap();
    assert_eq!(node.spec.title, "B");
    assert_eq!(node.kind, TASK);
    assert_eq!(node.metadata.backend, f.backend_name());
    assert_eq!(node.metadata.created_by, "tester");
    assert!(!node.metadata.etag.is_empty());
    let unpacked: wg::Task = node.spec.payload.unpack_unchecked().unwrap();
    assert_eq!(unpacked.touched_paths, ["src/lib.rs"]);

    let graphs = f.list_graphs().await.unwrap();
    assert_eq!(graphs.len(), 1);
    assert_eq!(graphs[0].node_count, 3);
    assert_eq!(graphs[0].root.r#ref, root);
}

async fn rejects_what_the_schema_forbids(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let b = item(f, &root, "B").await;
    let other = plan(f, "Q").await;
    let c = item(f, &other, "C").await;

    let bad = |r: crate::Result<wg::Node>| matches!(r, Err(Error::InvalidArgument(_)));
    // An unknown kind, a task at the root, a mismatched payload, no title.
    assert!(bad(f
        .create_node(
            None,
            "type.googleapis.com/x.Y",
            payload::spec("x", "", &wg::Plan::default()),
            HashMap::new(),
            &p
        )
        .await));
    assert!(bad(f
        .create_node(
            None,
            TASK,
            payload::spec("t", "", &task("", &[])),
            HashMap::new(),
            &p
        )
        .await));
    assert!(bad(f
        .create_node(
            Some(&root),
            TASK,
            payload::spec("t", "", &wg::Plan::default()),
            HashMap::new(),
            &p
        )
        .await));
    assert!(bad(f
        .create_node(
            Some(&root),
            TASK,
            payload::spec("  ", "", &task("", &[])),
            HashMap::new(),
            &p
        )
        .await));
    // A decision has no children.
    let d = f
        .create_node(
            Some(&root),
            DECISION,
            payload::spec("D", "", &wg::Decision::default()),
            HashMap::new(),
            &p,
        )
        .await
        .unwrap()
        .r#ref;
    assert!(bad(f
        .create_node(
            Some(&d),
            TASK,
            payload::spec("t", "", &task("", &[])),
            HashMap::new(),
            &p
        )
        .await));

    // Edges: the tree only through create and move; no self-edges; no edge
    // between graphs; no blocked_by from a plan; no cycles.
    assert!(matches!(
        f.add_edge(&edge(CHILD_OF, &a, &b), &p).await,
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        f.add_edge(&edge(BLOCKED_BY, &a, &a), &p).await,
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        f.add_edge(&edge(BLOCKED_BY, &a, &c), &p).await,
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        f.add_edge(&edge(BLOCKED_BY, &root, &a), &p).await,
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        f.add_edge(&edge("depends_on", &a, &b), &p).await,
        Err(Error::InvalidArgument(_))
    ));
    f.add_edge(&edge(BLOCKED_BY, &a, &b), &p).await.unwrap();
    assert!(matches!(
        f.add_edge(&edge(BLOCKED_BY, &a, &b), &p).await,
        Err(Error::AlreadyExists(_))
    ));
    assert!(matches!(
        f.add_edge(&edge(BLOCKED_BY, &b, &a), &p).await,
        Err(Error::FailedPrecondition(_))
    ));
    // A network edge may join any kinds, the plan included.
    f.add_edge(&edge(RELATES_TO, &root, &a), &p).await.unwrap();
    f.remove_edge(&edge(BLOCKED_BY, &a, &b), &p).await.unwrap();
    assert!(matches!(
        f.remove_edge(&edge(BLOCKED_BY, &a, &b), &p).await,
        Err(Error::NotFound(_))
    ));
}

async fn moves_within_the_hierarchy(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let b = item(f, &root, "B").await;
    let a1 = item(f, &a, "A1").await;
    let other = plan(f, "Q").await;
    let before = f.graph(&root).await.unwrap().revision;

    assert!(matches!(
        f.move_node(&root, &a, &p).await,
        Err(Error::FailedPrecondition(_))
    ));
    assert!(matches!(
        f.move_node(&a, &a1, &p).await,
        Err(Error::FailedPrecondition(_))
    ));
    assert!(matches!(
        f.move_node(&a, &other, &p).await,
        Err(Error::FailedPrecondition(_))
    ));

    f.move_node(&a1, &b, &p).await.unwrap();
    let g = f.graph(&root).await.unwrap().graph;
    let parents: Vec<&str> = g
        .edges
        .iter()
        .filter(|e| e.kind == CHILD_OF && e.from == a1)
        .map(|e| e.to.as_str())
        .collect();
    assert_eq!(parents, [b.as_str()], "exactly one parent after a move");

    let diff = f.diff(&root, Some(&before), None, true).await.unwrap();
    let moved = diff
        .iter()
        .find(|e| e.key == a1)
        .expect("the moved node is in the diff");
    assert_eq!(moved.change, Change::Moved);
    assert_eq!(moved.from_parent.as_deref(), Some(a.as_str()));
    assert_eq!(moved.to_parent.as_deref(), Some(b.as_str()));
    assert!(!moved.content_changed);
}

async fn refuses_stale_etags(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let first = f.node(&a).await.unwrap();
    let etag = first.metadata.etag.clone();
    let mut spec = first.spec.clone().into_option().unwrap();
    spec.title = "A, renamed".into();
    let updated = f
        .update_node(&a, Some(spec.clone()), None, Some(etag.clone()), &p)
        .await
        .unwrap();
    assert_ne!(updated.metadata.etag, etag);
    spec.title = "A, renamed twice".into();
    assert!(matches!(
        f.update_node(&a, Some(spec.clone()), None, Some(etag), &p)
            .await,
        Err(Error::Conflict(_))
    ));
    // Without an etag the update overwrites.
    f.update_node(&a, Some(spec), None, None, &p).await.unwrap();
    assert_eq!(f.node(&a).await.unwrap().spec.title, "A, renamed twice");
}

async fn derives_the_ready_set(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let b = item(f, &root, "B").await;
    let c = item(f, &root, "C").await;
    let c1 = item(f, &c, "C1").await;
    f.add_edge(&edge(BLOCKED_BY, &b, &a), &p).await.unwrap();

    let state = f.graph(&root).await.unwrap();
    assert!(!ready(&state.graph, &a), "proposed nodes are never ready");
    assert_eq!(
        condition(&state.graph, &a, condition::READY)
            .unwrap()
            .reason,
        "NotApproved"
    );

    f.approve(&root, &state.revision, &p).await.unwrap();
    let g = f.graph(&root).await.unwrap().graph;
    assert!(ready(&g, &a));
    assert!(!ready(&g, &b));
    assert!(condition(&g, &b, condition::BLOCKED).is_some_and(|c| c.status));
    assert!(
        !ready(&g, &c),
        "a node with open children isn't ready itself"
    );
    assert!(ready(&g, &c1));
    assert!(
        condition(&g, &root, condition::READY).is_none(),
        "the root isn't dispatched"
    );

    f.set_phase(&a, wg::Phase::PHASE_DONE, &p).await.unwrap();
    let g = f.graph(&root).await.unwrap().graph;
    assert!(ready(&g, &b), "a resolved blocker unblocks");
    assert!(!ready(&g, &a), "done nodes aren't ready");

    // Editing an approved node drifts it and pauses it.
    let mut spec = f.node(&c1).await.unwrap().spec.into_option().unwrap();
    spec.title = "C1, changed".into();
    f.update_node(&c1, Some(spec), None, None, &p)
        .await
        .unwrap();
    let state = f.graph(&root).await.unwrap();
    assert!(state.drifted.contains(&c1));
    assert!(condition(&state.graph, &c1, condition::DRIFTED).is_some_and(|c| c.status));
    assert!(!ready(&state.graph, &c1));
    assert!(ready(&state.graph, &b), "unaffected branches keep running");

    // Approving a stale revision is refused; the current one clears drift.
    assert!(matches!(
        f.approve(&root, "0000", &p).await,
        Err(Error::FailedPrecondition(_))
    ));
    f.approve(&root, &state.revision, &p).await.unwrap();
    let state = f.graph(&root).await.unwrap();
    assert!(state.drifted.is_empty());
    assert!(ready(&state.graph, &c1));
}

async fn records_revisions_for_content_only(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let b = item(f, &root, "B").await;
    let count = || async { f.revisions(&root).await.unwrap().len() };
    let n = count().await;
    assert_eq!(n, 3, "one revision per content change");

    f.add_edge(&edge(RELATES_TO, &a, &b), &p).await.unwrap();
    f.set_phase(&a, wg::Phase::PHASE_APPROVED, &p)
        .await
        .unwrap();
    f.annotate(&a, crate::schema::RECONCILE_NOTE, None, &p)
        .await
        .unwrap();
    assert_eq!(
        count().await,
        n,
        "unhashed edges, phases and annotations aren't content"
    );

    f.add_edge(&edge(BLOCKED_BY, &a, &b), &p).await.unwrap();
    assert_eq!(count().await, n + 1, "a blocker is content");

    let revs = f.revisions(&root).await.unwrap();
    for pair in revs.windows(2) {
        assert_eq!(
            pair[0].parent_root_hash, pair[1].root_hash,
            "newest first, chained"
        );
        assert_eq!(pair[0].sequence, pair[1].sequence + 1);
    }
    assert_eq!(revs.last().unwrap().parent_root_hash, "");
    assert_eq!(revs[0].provenance, p);
    assert_eq!(revs[0].changes.get("modified"), Some(&1));
    let section = revs[0]
        .sections
        .iter()
        .find(|s| s.key == a)
        .expect("every node has a text section");
    let lines: Vec<&str> = revs[0].text.lines().collect();
    assert!(lines[section.start_line as usize - 1].ends_with(": A"));
    assert!(revs[0].text.contains("blocked by: B"));

    // A node's own change against its descendants'.
    let a1 = item(f, &a, "A1").await;
    let before = f.graph(&root).await.unwrap().revision;
    let mut spec = f.node(&a1).await.unwrap().spec.into_option().unwrap();
    spec.title = "A1, edited".into();
    f.update_node(&a1, Some(spec), None, None, &p)
        .await
        .unwrap();
    let full = f.diff(&root, Some(&before), None, false).await.unwrap();
    let by_key: HashMap<&str, Change> = full.iter().map(|e| (e.key.as_str(), e.change)).collect();
    assert_eq!(by_key[a1.as_str()], Change::Modified);
    assert_eq!(by_key[a.as_str()], Change::ModifiedDescendants);
    assert!(!by_key.contains_key(b.as_str()));
    let compact = f.diff(&root, Some(&before), None, true).await.unwrap();
    assert_eq!(compact.len(), 1);

    // Against nothing, everything is added.
    let all = f.diff(&root, None, None, false).await.unwrap();
    assert!(all.iter().all(|e| e.change == Change::Added));
    assert_eq!(all.len(), 3);
}

async fn deletes(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let a1 = item(f, &a, "A1").await;
    let b = item(f, &root, "B").await;
    f.add_edge(&edge(BLOCKED_BY, &b, &a1), &p).await.unwrap();

    assert!(matches!(
        f.delete_node(&a, false, &p).await,
        Err(Error::FailedPrecondition(_))
    ));
    f.delete_node(&a, true, &p).await.unwrap();
    let g = f.graph(&root).await.unwrap().graph;
    assert_eq!(g.nodes.len(), 2);
    assert!(
        g.edges.iter().all(|e| e.from != a1 && e.to != a1),
        "edges go with their nodes"
    );
    assert!(matches!(f.node(&a1).await, Err(Error::NotFound(_))));

    f.delete_node(&root, true, &p).await.unwrap();
    assert!(f.list_graphs().await.unwrap().is_empty());
}

async fn keeps_annotations_and_runs(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let learning = wg::Learning {
        lesson: "one lesson".into(),
        evidence: vec!["https://example.com/pr/1".into()],
        ..Default::default()
    };
    assert!(matches!(
        f.annotate(
            &a,
            crate::schema::LEARNING,
            Some(payload::pack(&wg::Task::default())),
            &p
        )
        .await,
        Err(Error::InvalidArgument(_))
    ));
    f.annotate(
        &a,
        crate::schema::LEARNING,
        Some(payload::pack(&learning)),
        &p,
    )
    .await
    .unwrap();
    let notes = f.annotations(&a).await.unwrap();
    assert_eq!(notes.len(), 1);
    let back: wg::Learning = notes[0].payload.unpack_unchecked().unwrap();
    assert_eq!(back.lesson, "one lesson");
    assert_eq!(notes[0].provenance.actor, "tester");

    let state = f.graph(&root).await.unwrap();
    f.approve(&root, &state.revision, &p).await.unwrap();
    let live = wg::Run {
        r#ref: crate::olai::new_ref(),
        node: a.clone(),
        attempt: 1,
        state: wg::RunState::RUN_STATE_WORKING.into(),
        lease_expiry: buffa_types::google::protobuf::Timestamp::from_unix_secs(
            chrono::Utc::now().timestamp() + 600,
        )
        .into(),
        ..Default::default()
    };
    f.put_run(&live, &p).await.unwrap();
    let g = f.graph(&root).await.unwrap().graph;
    assert!(condition(&g, &a, condition::RUNNING).is_some_and(|c| c.status));
    assert!(!ready(&g, &a));

    let mut expired = live.clone();
    expired.lease_expiry = buffa_types::google::protobuf::Timestamp::from_unix_secs(
        chrono::Utc::now().timestamp() - 1,
    )
    .into();
    f.put_run(&expired, &p).await.unwrap();
    let g = f.graph(&root).await.unwrap().graph;
    assert!(condition(&g, &a, condition::STALLED).is_some_and(|c| c.status));
    assert_eq!(
        f.runs(&root).await.unwrap().len(),
        1,
        "put_run replaces by ref"
    );

    let kinds: Vec<String> = f
        .events(&root)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert!(kinds.contains(&"annotation".to_owned()));
    assert!(kinds.contains(&"approve".to_owned()));
    assert!(kinds.contains(&"run".to_owned()));
}

async fn keeps_run_steps_and_artifacts(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let state = f.graph(&root).await.unwrap();
    f.approve(&root, &state.revision, &p).await.unwrap();

    let session = wg::OmnigentSession {
        session_id: "sess-1".into(),
        spec: wg::ResolvedSessionSpec {
            profile: "executor".into(),
            agent_id: "ag_1".into(),
            labels: [("factory.step".to_owned(), "0".to_owned())].into(),
            ..Default::default()
        }
        .into(),
        status: "idle".into(),
        cost_usd: 1.5,
        ..Default::default()
    };
    let run = wg::Run {
        r#ref: crate::olai::new_ref(),
        node: a.clone(),
        attempt: 1,
        state: wg::RunState::RUN_STATE_WORKING.into(),
        lease_expiry: buffa_types::google::protobuf::Timestamp::from_unix_secs(
            chrono::Utc::now().timestamp() + 600,
        )
        .into(),
        executor: "task.v1".into(),
        stage: "in_review".into(),
        revision: state.revision.clone(),
        steps: vec![wg::Step {
            index: 0,
            kind: wg::StepKind::STEP_KIND_SESSION.into(),
            state: wg::StepState::STEP_STATE_SUCCEEDED.into(),
            name: "implement".into(),
            payload: payload::pack(&session).into(),
            ..Default::default()
        }],
        artifacts: vec![wg::Artifact {
            artifact: Some(wg::artifact::Artifact::PullRequest(Box::new(
                wg::PullRequest {
                    url: "https://github.com/o/r/pull/1".into(),
                    number: 1,
                    group: wg::Group {
                        name: "s".into(),
                        position: 1,
                        of: 2,
                        ..Default::default()
                    }
                    .into(),
                    ..Default::default()
                },
            ))),
            source: wg::ArtifactSource::ARTIFACT_SOURCE_REPORTED.into(),
            ..Default::default()
        }],
        cost_usd: 1.5,
        ..Default::default()
    };
    let mut rx = f.subscribe();
    f.put_run(&run, &p).await.unwrap();
    let change = rx.recv().await.unwrap();
    assert_eq!(
        change.run.as_ref(),
        Some(&run),
        "the change carries the run"
    );

    let back = f.run(&root, &run.r#ref).await.unwrap();
    assert_eq!(back, run, "steps and artifacts round-trip");
    let s: wg::OmnigentSession = back.steps[0].payload.unpack_unchecked().unwrap();
    assert_eq!(s.spec.agent_id, "ag_1");
    assert!(matches!(
        f.run(&root, "olai:00000000-0000-0000-0000-000000000000")
            .await,
        Err(Error::NotFound(_))
    ));

    let g = f.graph(&root).await.unwrap().graph;
    assert!(condition(&g, &a, condition::IN_REVIEW).is_some_and(|c| c.status));
    let mut waiting = run.clone();
    waiting.stage = "awaiting_merge".into();
    f.put_run(&waiting, &p).await.unwrap();
    let g = f.graph(&root).await.unwrap().graph;
    assert!(condition(&g, &a, condition::AWAITING_GATE).is_some_and(|c| c.status));
    assert!(condition(&g, &a, condition::IN_REVIEW).is_none());
}

async fn gates_block_like_items(f: &Factory) {
    let p = who();
    let root = plan(f, "P").await;
    let a = item(f, &root, "A").await;
    let gate = f
        .create_node(
            Some(&root),
            crate::schema::GATE,
            payload::spec(
                "released",
                "",
                &wg::Gate {
                    condition: payload::pack(&wg::HumanGate {
                        prompt: "check the release".into(),
                        ..Default::default()
                    })
                    .into(),
                    ..Default::default()
                },
            ),
            HashMap::new(),
            &p,
        )
        .await
        .unwrap()
        .r#ref;
    f.add_edge(&edge(BLOCKED_BY, &a, &gate), &p).await.unwrap();
    assert!(
        matches!(
            f.create_node(
                Some(&gate),
                TASK,
                payload::spec("under a gate", "", &wg::Task::default()),
                HashMap::new(),
                &p,
            )
            .await,
            Err(Error::InvalidArgument(_) | Error::FailedPrecondition(_))
        ),
        "a gate has no children"
    );
    let unregistered = wg::Gate {
        // Not a registered condition in any composition.
        condition: payload::pack(&wg::Learning::default()).into(),
        ..Default::default()
    };
    assert!(
        matches!(
            f.create_node(
                Some(&root),
                crate::schema::GATE,
                payload::spec("unknown", "", &unregistered),
                HashMap::new(),
                &p,
            )
            .await,
            Err(Error::InvalidArgument(_))
        ),
        "an unregistered gate condition is refused"
    );
    let state = f.graph(&root).await.unwrap();
    f.approve(&root, &state.revision, &p).await.unwrap();
    let g = f.graph(&root).await.unwrap().graph;
    assert!(!ready(&g, &a), "the gate blocks");
    f.set_phase(&gate, wg::Phase::PHASE_DONE, &p).await.unwrap();
    let g = f.graph(&root).await.unwrap().graph;
    assert!(ready(&g, &a), "a resolved gate unblocks");
}

async fn notifies_watchers(f: &Factory) {
    let p = who();
    let mut rx = f.subscribe();
    let root = plan(f, "P").await;
    let first = rx.recv().await.unwrap();
    assert_eq!(first.root, root);
    let a = item(f, &root, "A").await;
    let second = rx.recv().await.unwrap();
    assert!(second.sequence > first.sequence);
    assert_ne!(second.revision, first.revision);
    f.set_phase(&a, wg::Phase::PHASE_DONE, &p).await.unwrap();
    let third = rx.recv().await.unwrap();
    assert_eq!(third.revision, second.revision, "a phase isn't content");
    assert!(third.sequence > second.sequence);
}
