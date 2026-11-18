//! A sample plan with a history, for the standalone server's `--seed demo`
//! and for tests: revisions with added children, a reparent, an edited
//! acceptance, a new blocker, a decision, a cross-reference, an approval in
//! the middle, a gate, and a live run in review with a group of two PRs.
//!
//! Only the reference kinds and conditions: a composition's extensions add
//! their own payloads on top, seeding this first.

use std::collections::HashMap;

use buffa_types::google::protobuf::Timestamp;
use chrono::Utc;
use factory_proto::workgraph::v1 as wg;

use crate::payload::{self, task};
use crate::schema::{BLOCKED_BY, DISCOVERED_FROM, GATE, RELATES_TO};
use crate::{Factory, Provenance, Result};

fn edge(kind: &str, from: &str, to: &str) -> wg::Edge {
    wg::Edge {
        kind: kind.into(),
        from: from.into(),
        to: to.into(),
        ..Default::default()
    }
}

const REPO: &str = "open-lakehouse/luminescent-lakehouse";

fn ts(offset_secs: i64) -> Timestamp {
    Timestamp::from_unix_secs(Utc::now().timestamp() + offset_secs)
}

/// Seed the demo plan. Returns its root ref.
pub async fn seed(factory: &Factory) -> Result<String> {
    let planner = Provenance::new("factory-planner", "workgraph-mcp");
    let operator = Provenance::new("operator", "control-room");
    let no_labels = HashMap::new;

    let plan = factory
        .create_node(
            None,
            crate::schema::PLAN,
            payload::spec(
                "Typed work graph on olai-store",
                "Store the factory's plans in olai-store and show them in the control room.",
                &wg::Plan {
                    goal: "An approved plan runs end to end on olai-store, with no GitHub issues."
                        .into(),
                    rubric: vec![
                        "The conformance suite passes on every backend.".into(),
                        "Every revision names its actor.".into(),
                    ],
                    criteria: vec!["AGENTS.md".into()],
                    repos: vec![wg::Repo {
                        slug: REPO.into(),
                        default_branch: "main".into(),
                        criteria: "AGENTS.md".into(),
                        ..Default::default()
                    }],
                    planner_profile: "planner".into(),
                    ..Default::default()
                },
            ),
            no_labels(),
            &planner,
        )
        .await?;
    let root = plan.r#ref.clone();

    let add = |parent: String,
               title: &'static str,
               acceptance: &'static str,
               paths: &'static [&'static str]| {
        let planner = planner.clone();
        async move {
            factory
                .create_node(
                    Some(&parent),
                    crate::schema::TASK,
                    payload::spec(
                        title,
                        "",
                        &wg::Task {
                            repo: REPO.into(),
                            profile: "executor".into(),
                            review_profile: "reviewer".into(),
                            ..task(acceptance, paths)
                        },
                    ),
                    HashMap::new(),
                    &planner,
                )
                .await
        }
    };

    let protos = add(
        root.clone(),
        "Define factory.workgraph.v1",
        "The node envelope, edge kinds and schema are protos, and `buf lint` passes.",
        &["crates/factory-proto/proto/**"],
    )
    .await?;
    let core = add(
        root.clone(),
        "Build the core on olai-store",
        "Writes are validated against the schema and every change records a revision.",
        &["crates/factory-core/**"],
    )
    .await?;
    let server = add(
        root.clone(),
        "Serve factory.v1 over Connect",
        "The control room reads and edits plans only through FactoryService.",
        &["crates/factory-server/**"],
    )
    .await?;
    let ui = add(
        root.clone(),
        "Control-room UI",
        "Plans render as graphs, and any two revisions diff.",
        &["packages/factory-ui/**"],
    )
    .await?;
    factory
        .add_edge(&edge(BLOCKED_BY, &core.r#ref, &protos.r#ref), &planner)
        .await?;
    factory
        .add_edge(&edge(BLOCKED_BY, &server.r#ref, &core.r#ref), &planner)
        .await?;

    let decision = factory
        .create_node(
            Some(&root),
            crate::schema::DECISION,
            payload::spec(
                "Where do revisions live?",
                "",
                &wg::Decision {
                    question:
                        "Store revisions in the work-graph backend or a separate factory store?"
                            .into(),
                    options: vec!["same olai-store".into(), "separate SQLite".into()],
                    ..Default::default()
                },
            ),
            HashMap::new(),
            &planner,
        )
        .await?;
    factory
        .add_edge(&edge(BLOCKED_BY, &core.r#ref, &decision.r#ref), &planner)
        .await?;

    // The core waits until the protos are merged.
    let gate = factory
        .create_node(
            Some(&root),
            GATE,
            payload::spec(
                "The protos are merged",
                "The core builds on the generated types.",
                &wg::Gate {
                    condition: payload::pack(&wg::PrMerged {
                        task: protos.r#ref.clone(),
                        ..Default::default()
                    })
                    .into(),
                    ..Default::default()
                },
            ),
            HashMap::new(),
            &planner,
        )
        .await?;
    factory
        .add_edge(&edge(BLOCKED_BY, &core.r#ref, &gate.r#ref), &planner)
        .await?;

    // The operator reviews and approves this revision.
    let current = factory.graph(&root).await?.revision;
    factory.approve(&root, &current, &operator).await?;
    factory
        .set_phase(&protos.r#ref, wg::Phase::PHASE_DONE, &operator)
        .await?;
    factory
        .set_phase(&gate.r#ref, wg::Phase::PHASE_DONE, &operator)
        .await?;
    let mut decided = factory.node(&decision.r#ref).await?;
    if let Some(spec) = decided.spec.as_option_mut() {
        spec.payload = payload::pack(&wg::Decision {
            question: "Store revisions in the work-graph backend or a separate factory store?"
                .into(),
            options: vec!["same olai-store".into(), "separate SQLite".into()],
            outcome: "same olai-store".into(),
            ..Default::default()
        })
        .into();
    }
    factory
        .update_node(
            &decision.r#ref,
            decided.spec.into_option(),
            None,
            None,
            &operator,
        )
        .await?;
    factory
        .set_phase(&decision.r#ref, wg::Phase::PHASE_DONE, &operator)
        .await?;

    // An executor works on the core and amends the plan inside its own item.
    let executor = Provenance {
        actor: "factory-executor".into(),
        via: "workgraph-mcp".into(),
        run_ref: crate::olai::new_ref(),
    };
    factory
        .put_run(
            &demo_run(&executor.run_ref, &core.r#ref, &root, &current),
            &executor,
        )
        .await?;
    let adapter = add(
        core.r#ref.clone(),
        "olai-store adapter",
        "Nodes are objects named [graph, node]; edges are associations.",
        &["crates/factory-core/src/olai.rs"],
    )
    .await?;
    add(
        core.r#ref.clone(),
        "Revision log",
        "Revisions chain by parent root hash.",
        &["crates/factory-core/src/factory.rs"],
    )
    .await?;
    factory
        .annotate(
            &core.r#ref,
            crate::schema::LEARNING,
            Some(payload::pack(&wg::Learning {
                lesson: "olai-store deletes an object's associations with it, so deleting a subtree needs no edge cleanup.".into(),
                evidence: vec!["trestle/crates/olai-store/src/store.rs".into()],
                ..Default::default()
            })),
            &executor,
        )
        .await?;

    // Out of scope for the executor: a new sibling task, a moved item, and a
    // sharper acceptance on the server task. These drift from the approval.
    let schema_task = add(
        root.clone(),
        "Serve the schema",
        "GetSchema returns every node and edge kind.",
        &["crates/factory-server/src/service.rs"],
    )
    .await?;
    factory
        .move_node(&schema_task.r#ref, &server.r#ref, &planner)
        .await?;
    let mut sharper = factory.node(&server.r#ref).await?;
    if let Some(spec) = sharper.spec.as_option_mut() {
        spec.payload = payload::pack(&task(
            "The control room reads and edits plans only through FactoryService, over Connect JSON and binary.",
            &["crates/factory-server/**"],
        ))
        .into();
    }
    factory
        .update_node(
            &server.r#ref,
            sharper.spec.into_option(),
            None,
            None,
            &planner,
        )
        .await?;
    factory
        .add_edge(&edge(BLOCKED_BY, &ui.r#ref, &server.r#ref), &planner)
        .await?;
    factory
        .add_edge(
            &edge(DISCOVERED_FROM, &adapter.r#ref, &core.r#ref),
            &executor,
        )
        .await?;
    factory
        .add_edge(&edge(RELATES_TO, &ui.r#ref, &decision.r#ref), &planner)
        .await?;
    Ok(root)
}

/// A run in review: the implementing session went idle with a group of two
/// PRs, and the review session is working.
fn demo_run(run_ref: &str, node: &str, root: &str, revision: &str) -> wg::Run {
    let labels = |step: u32| {
        HashMap::from([
            ("factory.graph".to_owned(), root.to_owned()),
            ("factory.node".to_owned(), node.to_owned()),
            ("factory.run".to_owned(), run_ref.to_owned()),
            ("factory.step".to_owned(), step.to_string()),
            ("factory.revision".to_owned(), revision.to_owned()),
        ])
    };
    let session = |id: &str,
                   profile: &str,
                   agent: &str,
                   harness: &str,
                   model: &str,
                   status: &str,
                   cost: f64,
                   step: u32| {
        wg::OmnigentSession {
            session_id: id.into(),
            spec: wg::ResolvedSessionSpec {
                profile: profile.into(),
                agent_id: format!("ag_{agent}"),
                agent_name: agent.into(),
                agent_version: "4".into(),
                harness: harness.into(),
                model: model.into(),
                host_type: "external".into(),
                host_id: "local".into(),
                workspace: "/workspace/luminescent-lakehouse".into(),
                branch: "factory/olai-core".into(),
                base_branch: "main".into(),
                prompt_name: "factory.dispatch.executor".into(),
                prompt_version: "3".into(),
                labels: labels(step),
                environment: "local".into(),
                placement_source: wg::PlacementSource::PLACEMENT_SOURCE_REGISTRY.into(),
                ..Default::default()
            }
            .into(),
            status: status.into(),
            cost_usd: cost,
            ..Default::default()
        }
    };
    let step = |index: u32,
                name: &str,
                state: wg::StepState,
                payload: &wg::OmnigentSession,
                start: i64,
                end: Option<i64>| wg::Step {
        index,
        kind: wg::StepKind::STEP_KIND_SESSION.into(),
        state: state.into(),
        name: name.into(),
        payload: payload::pack(payload).into(),
        started_at: ts(start).into(),
        ended_at: end.map(ts).into(),
        ..Default::default()
    };
    let pr = |number: u32, layer: u32, title: &str, base: &str| wg::Artifact {
        artifact: Some(wg::artifact::Artifact::PullRequest(Box::new(
            wg::PullRequest {
                url: format!("https://github.com/{REPO}/pull/{number}"),
                repo: REPO.into(),
                number,
                title: title.into(),
                head: format!("factory/olai-core-{layer}"),
                base: base.into(),
                state: "open".into(),
                checks: "passing".into(),
                review_decision: "review_required".into(),
                group: wg::Group {
                    name: "olai-core".into(),
                    position: layer,
                    of: 2,
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
        ))),
        step: 0,
        source: wg::ArtifactSource::ARTIFACT_SOURCE_REPORTED.into(),
        ..Default::default()
    };
    let implement = session(
        "sess-demo-1",
        "executor",
        "factory-executor",
        "claude-sdk",
        "opus",
        "idle",
        3.2,
        0,
    );
    let review = session(
        "sess-demo-2",
        "reviewer",
        "factory-reviewer",
        "codex",
        "gpt-5.5",
        "running",
        0.4,
        1,
    );
    wg::Run {
        r#ref: run_ref.into(),
        node: node.into(),
        attempt: 1,
        state: wg::RunState::RUN_STATE_WORKING.into(),
        session_id: "sess-demo-2".into(),
        lease_expiry: ts(3600).into(),
        prompt_name: "factory.dispatch.executor".into(),
        prompt_version: "3".into(),
        created_at: ts(-1800).into(),
        updated_at: ts(-60).into(),
        executor: "task.v1".into(),
        stage: "in_review".into(),
        revision: revision.into(),
        steps: vec![
            step(
                0,
                "implement",
                wg::StepState::STEP_STATE_SUCCEEDED,
                &implement,
                -1800,
                Some(-300),
            ),
            step(
                1,
                "review",
                wg::StepState::STEP_STATE_ACTIVE,
                &review,
                -240,
                None,
            ),
        ],
        artifacts: vec![
            pr(
                101,
                1,
                "feat(factory): store work graphs in olai-store",
                "main",
            ),
            pr(
                102,
                2,
                "feat(factory): record a revision on every write",
                "factory/olai-core-1",
            ),
        ],
        cost_usd: 3.6,
        ..Default::default()
    }
}
