//! FactoryService end to end: the router on a real port over an olai-store
//! backend, driven by the generated Connect client.

use std::sync::Arc;

use connectrpc::ErrorCode;
use connectrpc::client::{ClientConfig, HttpClient};
use factory_core::extension::{Declared, Extension, GateEvaluator, KindSpec};
use factory_core::payload::{self, task};
use factory_core::schema::{BLOCKED_BY, PLAN, RELATES_TO, TASK};
use factory_core::{Factory, FactoryBuilder, Provenance, Registry, olai};
use factory_proto::connect_gen::factory::v1::FactoryServiceClient;
use factory_proto::v1;
use factory_proto::workgraph::v1 as wg;

type Client = FactoryServiceClient<HttpClient>;

async fn serve() -> Client {
    serve_with(Registry::default()).await.0
}

async fn serve_with(registry: Registry) -> (Client, Arc<Factory>) {
    serve_built(FactoryBuilder::new(), registry).await
}

async fn serve_built(builder: FactoryBuilder, registry: Registry) -> (Client, Arc<Factory>) {
    let schema = builder.schema().unwrap();
    let backend = olai::sqlite("sqlite::memory:", &schema).await.unwrap();
    let factory = Arc::new(
        builder
            .build(Arc::new(backend))
            .unwrap()
            .with_registry(registry),
    );
    let served = factory.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, factory_server::router(served))
            .await
            .unwrap();
    });
    let uri = format!("http://{addr}").parse().unwrap();
    (
        FactoryServiceClient::new(HttpClient::plaintext(), ClientConfig::new(uri)),
        factory,
    )
}

async fn create(c: &Client, parent: &str, kind: &str, spec: wg::NodeSpec) -> String {
    c.create_node(v1::CreateNodeRequest {
        parent: parent.into(),
        kind: kind.into(),
        spec: spec.into(),
        ..Default::default()
    })
    .await
    .unwrap()
    .into_owned()
    .node
    .r#ref
    .clone()
}

fn edge(kind: &str, from: &str, to: &str) -> wg::Edge {
    wg::Edge {
        kind: kind.into(),
        from: from.into(),
        to: to.into(),
        ..Default::default()
    }
}

async fn revision(c: &Client, root: &str) -> String {
    c.get_graph(v1::GetGraphRequest {
        root: root.into(),
        ..Default::default()
    })
    .await
    .unwrap()
    .into_owned()
    .revision
}

/// An extension the server has no code for: a node kind and a gate
/// condition, from messages outside the reference set.
struct Notes;

impl Extension for Notes {
    fn name(&self) -> &'static str {
        "notes"
    }
    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec::of::<wg::Learning>(wg::NodeKind {
            display_name: "Note".into(),
            ..Default::default()
        })]
    }
    fn gates(&self) -> Vec<Box<dyn GateEvaluator>> {
        vec![Box::new(Declared::<wg::DecisionOutcome>::new(
            "Decided",
            "A decision has an outcome.",
        ))]
    }
}

#[tokio::test]
async fn schema_lists_what_extensions_register() {
    let (c, _) = serve_built(FactoryBuilder::new().extension(Notes), Registry::default()).await;
    let resp = c
        .get_schema(v1::GetSchemaRequest::default())
        .await
        .unwrap()
        .into_owned();
    let schema = resp.schema.as_option().unwrap();
    let note = schema
        .node_kinds
        .iter()
        .find(|k| k.kind == wg::Learning::TYPE_URL)
        .expect("the extension's kind");
    assert_eq!(note.extension, "notes");
    let task = schema.node_kinds.iter().find(|k| k.kind == TASK).unwrap();
    assert_eq!(task.extension, "builtin");
    assert!(task.display.as_option().is_some_and(|d| !d.icon.is_empty()));
    let decided = schema
        .gate_conditions
        .iter()
        .find(|k| k.type_url == wg::DecisionOutcome::TYPE_URL)
        .expect("the extension's gate condition");
    assert_eq!(decided.display_name, "Decided");
    assert!(
        schema
            .gate_conditions
            .iter()
            .any(|k| k.type_url == wg::PrMerged::TYPE_URL && k.extension == "builtin")
    );
    let set = buffa_descriptor::DescriptorPool::decode(&resp.file_descriptor_set).unwrap();
    assert!(set.message_by_name("factory.workgraph.v1.Task").is_some());
    assert!(set.message_by_name("google.protobuf.Timestamp").is_some());
}

#[tokio::test]
async fn plans_edits_and_revisions() {
    let c = serve().await;

    let schema = c
        .get_schema(v1::GetSchemaRequest::default())
        .await
        .unwrap()
        .into_owned();
    assert_eq!(
        schema.schema.node_kinds.len(),
        4,
        "Plan, Task, Decision, Gate"
    );

    let root = create(&c, "", PLAN, payload::spec("P", "", &wg::Plan::default())).await;
    let a = create(
        &c,
        &root,
        TASK,
        payload::spec("A", "", &task("a", &["a.rs"])),
    )
    .await;
    let b = create(
        &c,
        &root,
        TASK,
        payload::spec("B", "", &task("b", &["b.rs"])),
    )
    .await;
    let a1 = create(&c, &a, TASK, payload::spec("A1", "", &task("a1", &[]))).await;

    // Schema rejections come back with their Connect codes.
    let err = c
        .create_node(v1::CreateNodeRequest {
            parent: root.clone(),
            kind: TASK.into(),
            spec: payload::spec("bad", "", &wg::Plan::default()).into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    c.add_edge(v1::AddEdgeRequest {
        edge: edge(BLOCKED_BY, &b, &a).into(),
        ..Default::default()
    })
    .await
    .unwrap();
    let err = c
        .add_edge(v1::AddEdgeRequest {
            edge: edge(BLOCKED_BY, &a, &b).into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::FailedPrecondition, "a cycle");

    // Approve, then change the plan in three ways.
    let approved = revision(&c, &root).await;
    c.approve_revision(v1::ApproveRevisionRequest {
        root: root.clone(),
        root_hash: approved.clone(),
        ..Default::default()
    })
    .await
    .unwrap();
    c.move_node(v1::MoveNodeRequest {
        r#ref: a1.clone(),
        new_parent: b.clone(),
        ..Default::default()
    })
    .await
    .unwrap();
    let node = c
        .get_graph(v1::GetGraphRequest {
            root: root.clone(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .graph
        .nodes
        .iter()
        .find(|n| n.r#ref == a)
        .cloned()
        .unwrap();
    let mut spec = node.spec.clone().into_option().unwrap();
    spec.title = "A, sharper".into();
    c.update_node(v1::UpdateNodeRequest {
        r#ref: a.clone(),
        spec: spec.clone().into(),
        etag: node.metadata.etag.clone(),
        ..Default::default()
    })
    .await
    .unwrap();
    let err = c
        .update_node(v1::UpdateNodeRequest {
            r#ref: a.clone(),
            spec: spec.into(),
            etag: node.metadata.etag.clone(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Aborted, "a stale etag");

    // A network edge isn't plan content.
    let before = c
        .list_revisions(v1::ListRevisionsRequest {
            root: root.clone(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .revisions
        .len();
    c.add_edge(v1::AddEdgeRequest {
        edge: edge(RELATES_TO, &a, &b).into(),
        ..Default::default()
    })
    .await
    .unwrap();
    let revisions = c
        .list_revisions(v1::ListRevisionsRequest {
            root: root.clone(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .revisions;
    assert_eq!(revisions.len(), before);
    assert!(
        revisions
            .iter()
            .any(|r| r.approved && r.root_hash == approved)
    );
    assert_eq!(revisions[0].parent_root_hash, revisions[1].root_hash);
    assert_eq!(revisions[0].provenance.actor, "operator");

    // The diff against the approval, and the drift it implies.
    let diff = c
        .diff_revisions(v1::DiffRevisionsRequest {
            root: root.clone(),
            base: approved.clone(),
            compact: true,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .entries;
    let change = |key: &str| {
        diff.iter()
            .find(|e| e.key == key)
            .map(|e| e.change.as_known().unwrap())
    };
    assert_eq!(change(&a1), Some(v1::ChangeKind::CHANGE_KIND_MOVED));
    assert_eq!(change(&a), Some(v1::ChangeKind::CHANGE_KIND_MODIFIED));
    let moved = diff.iter().find(|e| e.key == a1).unwrap();
    assert_eq!(
        (moved.from_parent.as_str(), moved.to_parent.as_str()),
        (a.as_str(), b.as_str())
    );

    let state = c
        .get_graph(v1::GetGraphRequest {
            root: root.clone(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned();
    assert_eq!(state.approved_revision, approved);
    assert!(state.drifted.contains(&a));

    let rev = c
        .get_revision(v1::GetRevisionRequest {
            root: root.clone(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned();
    assert_eq!(rev.tree.subtree_hash, state.revision);
    assert!(rev.text.contains("## Task: A, sharper"));
    assert!(rev.sections.iter().any(|s| s.key == a1));
}

#[tokio::test]
async fn watch_streams_changes() {
    let c = serve().await;
    let root = create(&c, "", PLAN, payload::spec("P", "", &wg::Plan::default())).await;
    let mut stream = c
        .watch_graph(v1::WatchGraphRequest {
            root: root.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
    let first = stream.message().await.unwrap().unwrap().to_owned_message();
    assert_eq!(first.revision, revision(&c, &root).await);

    create(&c, &root, TASK, payload::spec("A", "", &task("a", &[]))).await;
    let next = stream.message().await.unwrap().unwrap().to_owned_message();
    assert_ne!(next.revision, first.revision);
    assert!(next.sequence > first.sequence);
}

#[tokio::test]
async fn registry_runs_and_unbuilt_rpcs() {
    let registry =
        Registry::from_yaml(include_str!("../../factory-core/registry.example.yaml")).unwrap();
    let (c, factory) = serve_with(registry).await;

    let profiles = c
        .list_profiles(v1::ListProfilesRequest::default())
        .await
        .unwrap()
        .into_owned()
        .profiles;
    let executor = profiles.iter().find(|p| p.name == "executor").unwrap();
    assert_eq!(executor.agent, "factory-executor");
    let repos = c
        .list_repos(v1::ListReposRequest::default())
        .await
        .unwrap()
        .into_owned()
        .repos;
    assert!(
        repos
            .iter()
            .any(|r| r.slug == "open-lakehouse/luminescent-lakehouse")
    );

    let root = factory_core::demo::seed(&factory).await.unwrap();
    let runs = c
        .list_runs(v1::ListRunsRequest {
            root: root.clone(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .runs;
    let run = c
        .get_run(v1::GetRunRequest {
            root: root.clone(),
            run: runs[0].r#ref.clone(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .run
        .into_option()
        .unwrap();
    assert_eq!(run.stage, "in_review");
    assert_eq!(run.steps.len(), 2);
    assert_eq!(run.artifacts.len(), 2);

    let err = c
        .start_run(v1::StartRunRequest {
            node: run.node.clone(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unimplemented);

    // A run update reaches watchers with the run attached.
    let mut stream = c
        .watch_graph(v1::WatchGraphRequest {
            root: root.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
    stream.message().await.unwrap().unwrap();
    let mut fixing = run.clone();
    fixing.stage = "fixing".into();
    factory
        .put_run(&fixing, &Provenance::new("reconciler", "test"))
        .await
        .unwrap();
    let next = stream.message().await.unwrap().unwrap().to_owned_message();
    match next.change {
        Some(v1::watch_graph_response::Change::Run(r)) => assert_eq!(r.stage, "fixing"),
        other => panic!("expected a run change, got {other:?}"),
    }
}

#[tokio::test]
async fn placements_are_chosen_at_submission() {
    let registry =
        Registry::from_yaml(include_str!("../../factory-core/registry.example.yaml")).unwrap();
    let (c, factory) = serve_with(registry).await;
    let envs = c
        .list_environments(v1::ListEnvironmentsRequest::default())
        .await
        .unwrap()
        .into_owned();
    assert_eq!(envs.default_environment, "local");
    assert!(envs.environments.iter().any(|e| e.name == "sandbox"));

    let root = factory_core::demo::seed(&factory).await.unwrap();
    let resolve = |environment: &str| {
        let c = c.clone();
        let root = root.clone();
        let environment = environment.to_owned();
        async move {
            c.resolve_placements(v1::ResolvePlacementsRequest {
                root,
                choice: wg::PlacementChoice {
                    environment,
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            })
            .await
            .unwrap()
            .into_owned()
        }
    };
    let local = resolve("").await;
    assert!(local.placeable, "{:?}", local.placements);
    let exec = local
        .placements
        .iter()
        .find(|p| p.role == "executor")
        .unwrap();
    assert_eq!(exec.environment, "local");
    assert!(exec.requires.contains(&"gh-stack".to_owned()));
    let sandbox = resolve("sandbox").await;
    assert!(
        sandbox
            .placements
            .iter()
            .filter(|p| p.role == "executor")
            .all(|p| p.environment == "sandbox")
    );

    // Execution is refused against a stale revision, and a strict per-node
    // choice that misses a requirement names the node.
    let state = factory.graph(&root).await.unwrap();
    let err = c
        .start_execution(v1::StartExecutionRequest {
            root: root.clone(),
            revision: "stale".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::FailedPrecondition);
    let approved = state.approved.clone().unwrap();
    let task = exec.node.clone();
    let err = c
        .start_execution(v1::StartExecutionRequest {
            root: root.clone(),
            revision: approved.clone(),
            choice: wg::PlacementChoice {
                nodes: [(task.clone(), "nowhere".to_owned())].into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::FailedPrecondition);
    assert!(err.message.as_deref().unwrap_or_default().contains(&task));
    let err = c
        .start_execution(v1::StartExecutionRequest {
            root,
            revision: approved,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::Unimplemented,
        "placeable, but not built yet"
    );
}
