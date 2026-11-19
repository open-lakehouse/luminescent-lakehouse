//! workgraph-mcp end to end: the demo plan on the in-memory backend, served
//! over Streamable HTTP behind bearer tokens, and driven by rmcp's client,
//! as an agent (and the desktop's side channel) would.

use std::sync::Arc;

use async_trait::async_trait;
use factory_core::tool::{ToolCall, ToolHandler, ToolOutput, ToolSpec};
use factory_core::{Extension, Factory, FactoryBuilder, condition, olai};
use factory_proto::workgraph::v1 as wg;
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, ContentBlock,
    ExtensionCapabilities, Implementation,
};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::{Value, json};
use workgraph_mcp::{Grant, Tokens, ToolPolicy, UI_EXTENSION, WorkgraphMcp, http};

// An extension that adds a tool for the model and one for an MCP App.

struct Whoami;
#[async_trait]
impl ToolHandler for Whoami {
    async fn call(&self, call: ToolCall<'_>) -> factory_core::Result<ToolOutput> {
        Ok(ToolOutput::json(json!({
            "actor": call.caller.actor,
            "role": call.caller.role,
            "graphs": call.factory.list_graphs().await?.len(),
            "arguments": call.arguments,
        })))
    }
}

struct Audit;
impl Extension for Audit {
    fn name(&self) -> &'static str {
        "audit"
    }
    fn mcp_tools(&self) -> Vec<ToolSpec> {
        let any = json!({ "type": "object" });
        vec![
            ToolSpec::new("audit_whoami", "Who's calling.", any.clone(), Whoami).read_only(),
            ToolSpec::new("audit_approve", "Approve, from the app.", any, Whoami)
                .app_only()
                .ui("ui://audit/view"),
        ]
    }
}

struct Clash;
impl Extension for Clash {
    fn name(&self) -> &'static str {
        "clash"
    }
    fn mcp_tools(&self) -> Vec<ToolSpec> {
        vec![ToolSpec::new("plan_read", "", json!({}), Whoami)]
    }
}

struct Served {
    url: String,
    tokens: Tokens,
    factory: Arc<Factory>,
    root: String,
    run: wg::Run,
}

async fn factory(builder: FactoryBuilder) -> Arc<Factory> {
    let backend = Arc::new(olai::in_memory(&builder.schema().unwrap()));
    Arc::new(builder.build(backend).unwrap())
}

async fn serve() -> Served {
    let factory = factory(FactoryBuilder::new().extension(Audit)).await;
    let root = factory_core::demo::seed(&factory).await.unwrap();
    let run = factory.runs(&root).await.unwrap().remove(0);
    let tokens = Tokens::new();
    let server = WorkgraphMcp::new(factory.clone()).unwrap();
    let router = http::router(server, tokens.clone(), http::PATH, http::config(&[]));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await });
    Served {
        url: format!("http://{addr}{}", http::PATH),
        tokens,
        factory,
        root,
        run,
    }
}

type Client = RunningService<RoleClient, ClientConfig>;

fn client_config(apps: bool) -> ClientConfig {
    let capabilities = if apps {
        let mut extensions = ExtensionCapabilities::new();
        extensions.insert(
            UI_EXTENSION.into(),
            serde_json::from_value(json!({ "mimeTypes": ["text/html;profile=mcp-app"] })).unwrap(),
        );
        ClientCapabilities::builder()
            .enable_extensions_with(extensions)
            .build()
    } else {
        ClientCapabilities::default()
    };
    ClientConfig::new(capabilities, Implementation::new("test-agent", "0"))
}

async fn connect(served: &Served, token: &str, apps: bool) -> Client {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(served.url.as_str()).auth_header(token),
    );
    client_config(apps).serve(transport).await.unwrap()
}

async fn names(client: &Client) -> Vec<String> {
    let mut names: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    names.sort();
    names
}

/// Every result is text plus structured content: never a resource.
fn checked(result: CallToolResult) -> CallToolResult {
    for block in &result.content {
        assert!(
            matches!(block, ContentBlock::Text(_)),
            "only text content reaches a client: {block:?}"
        );
    }
    result
}

async fn try_call(
    client: &Client,
    name: &str,
    args: Value,
) -> Result<CallToolResult, rmcp::ServiceError> {
    let Value::Object(args) = args else {
        panic!("arguments are an object")
    };
    client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(args))
        .await
        .map(checked)
}

/// A successful call's structured result.
async fn call(client: &Client, name: &str, args: Value) -> Value {
    let result = try_call(client, name, args).await.unwrap();
    assert_ne!(result.is_error, Some(true), "{name}: {:?}", result.content);
    result.structured_content.expect("structured content")
}

/// A call that fails as a tool error; returns its text.
async fn call_err(client: &Client, name: &str, args: Value) -> String {
    let result = try_call(client, name, args).await.unwrap();
    assert_eq!(result.is_error, Some(true), "{name} should fail");
    result
        .content
        .iter()
        .filter_map(ContentBlock::as_text)
        .map(|t| t.text.clone())
        .collect()
}

const BUILTIN: [&str; 11] = [
    "add_edge",
    "add_item",
    "annotate",
    "item_read",
    "list_profiles",
    "list_repos",
    "plan_read",
    "propose",
    "ready",
    "report_change",
    "submit",
];

#[tokio::test]
async fn requests_need_a_live_token() {
    let served = serve().await;
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": { "name": "curl", "version": "0" } }
    });
    let post = |token: Option<&str>| {
        let mut req = reqwest::Client::new()
            .post(&served.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(init.to_string());
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        req.send()
    };
    for token in [None, Some("nope")] {
        let res = post(token).await.unwrap();
        assert_eq!(res.status(), 401, "{token:?}");
        let challenge = res.headers()["www-authenticate"].to_str().unwrap();
        assert!(challenge.starts_with("Bearer "), "{challenge}");
    }
    let token = served
        .tokens
        .mint(Grant::new("operator", ToolPolicy::all()));
    assert_eq!(post(Some(&token)).await.unwrap().status(), 200);

    // Revoking a token ends its session's access at once.
    let client = connect(&served, &token, false).await;
    call(&client, "list_repos", json!({})).await;
    assert!(served.tokens.revoke(&token));
    assert!(try_call(&client, "list_repos", json!({})).await.is_err());
}

#[tokio::test]
async fn the_policy_decides_what_a_token_sees() {
    let served = serve().await;
    let list = async |policy: &str, apps: bool| {
        let grant = Grant::new("someone", policy.parse().unwrap()).role("whatever");
        let token = served.tokens.mint(grant);
        names(&connect(&served, &token, apps).await).await
    };

    let mut everything: Vec<String> = BUILTIN.iter().map(|s| (*s).to_owned()).collect();
    everything.push("audit_whoami".into());
    everything.sort();
    assert_eq!(list("*", false).await, everything);

    // App-only tools exist only for clients that declare MCP Apps.
    let mut with_app = everything.clone();
    with_app.push("audit_approve".into());
    with_app.sort();
    assert_eq!(list("*", true).await, with_app);

    assert_eq!(
        list("@read", true).await,
        [
            "audit_whoami",
            "item_read",
            "list_profiles",
            "list_repos",
            "plan_read",
            "ready"
        ]
    );
    assert_eq!(
        list("plan_read,ready,report_change", false).await,
        ["plan_read", "ready", "report_change"]
    );
    assert_eq!(
        list("audit_*", true).await,
        ["audit_approve", "audit_whoami"]
    );
    assert_eq!(
        list("@write,!add_*,!audit_*", true).await,
        ["annotate", "propose", "report_change", "submit"]
    );
    assert!(
        list("", false).await.is_empty(),
        "an empty policy allows nothing"
    );

    // A tool outside the policy doesn't exist for the caller.
    let token = served
        .tokens
        .mint(Grant::new("reader", ToolPolicy::read_only()));
    let client = connect(&served, &token, false).await;
    let err = try_call(&client, "add_item", json!({})).await.unwrap_err();
    assert!(err.to_string().contains("unknown tool"), "{err}");
    let err = try_call(&client, "audit_approve", json!({}))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("unknown tool"), "{err}");

    // An extension's tool runs with the caller's grant.
    let who = call(&client, "audit_whoami", json!({ "x": 1 })).await;
    assert_eq!(who["actor"], "reader");
    assert_eq!(who["arguments"]["x"], 1);
}

#[tokio::test]
async fn app_tools_carry_their_ui_metadata() {
    let served = serve().await;
    let token = served
        .tokens
        .mint(Grant::new("operator", ToolPolicy::all()));
    let client = connect(&served, &token, true).await;
    let tools = client.list_all_tools().await.unwrap();
    let approve = tools.iter().find(|t| t.name == "audit_approve").unwrap();
    let meta = serde_json::to_value(approve.meta.as_ref().unwrap()).unwrap();
    assert_eq!(meta["ui"]["visibility"], json!(["app"]));
    assert_eq!(meta["ui"]["resourceUri"], "ui://audit/view");
    let read = tools.iter().find(|t| t.name == "plan_read").unwrap();
    assert_eq!(
        read.annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );
    assert!(read.meta.is_none());
    call(&client, "audit_approve", json!({})).await;
}

#[tokio::test]
async fn an_agent_reads_and_amends_its_plan() {
    let served = serve().await;
    let (root, run) = (&served.root, &served.run);
    let grant = Grant::new("factory-executor", ToolPolicy::all())
        .role("executor")
        .root(root.clone())
        .run(run.r#ref.clone())
        .node(run.node.clone());
    let client = connect(&served, &served.tokens.mint(grant), false).await;

    // Reads default to the session's plan.
    let plan = call(&client, "plan_read", json!({})).await;
    assert_eq!(plan["root"], root.as_str());
    let state = served.factory.graph(root).await.unwrap();
    assert_eq!(
        plan["items"].as_array().unwrap().len(),
        state.graph.nodes.len()
    );
    assert_eq!(plan["revision"], state.revision.as_str());
    assert!(
        plan["edges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["kind"] != "child_of")
    );

    let ready = call(&client, "ready", json!({})).await;
    let expected = state
        .graph
        .nodes
        .iter()
        .filter(|n| {
            n.status
                .conditions
                .iter()
                .any(|c| c.r#type == condition::READY && c.status)
        })
        .count();
    assert_eq!(ready["ready"].as_array().unwrap().len(), expected);

    let item = call(&client, "item_read", json!({ "ref": run.node })).await;
    assert_eq!(item["item"]["ref"], run.node.as_str());
    assert_eq!(item["runs"][0]["ref"], run.r#ref.as_str());

    // add_item lands proposed, attributed to the session's run.
    let added = call(
        &client,
        "add_item",
        json!({
            "parent": root,
            "kind": "task",
            "title": "Document the MCP server",
            "payload": { "acceptance": "docs/factory/workgraph-mcp.md exists" }
        }),
    )
    .await;
    let new_ref = added["item"]["ref"].as_str().unwrap().to_owned();
    let node = served.factory.node(&new_ref).await.unwrap();
    assert_eq!(
        node.status.phase.as_known(),
        Some(wg::Phase::PHASE_PROPOSED)
    );
    let wg::Task { acceptance, .. } = node
        .spec
        .payload
        .unpack_if::<wg::Task>(factory_core::schema::TASK)
        .unwrap()
        .unwrap();
    assert_eq!(acceptance, "docs/factory/workgraph-mcp.md exists");
    let latest = served.factory.revision(root, None).await.unwrap();
    assert_eq!(latest.provenance.actor, "factory-executor");
    assert_eq!(latest.provenance.via, "workgraph-mcp");
    assert_eq!(latest.provenance.run_ref, run.r#ref);

    call(
        &client,
        "add_edge",
        json!({ "kind": "blocked_by", "from": new_ref, "to": run.node }),
    )
    .await;
    let plan = call(&client, "plan_read", json!({})).await;
    assert!(
        plan["edges"]
            .as_array()
            .unwrap()
            .contains(&json!({ "kind": "blocked_by", "from": new_ref, "to": run.node })),
        "{plan}"
    );

    let note = call(
        &client,
        "annotate",
        json!({
            "node": new_ref, "kind": "learning",
            "payload": { "lesson": "Tokens are per session.", "evidence": ["test"] }
        }),
    )
    .await;
    assert_eq!(note["annotation"]["kind"], factory_core::schema::LEARNING);
    let err = call_err(
        &client,
        "add_item",
        json!({ "parent": root, "kind": "Nope", "title": "x" }),
    )
    .await;
    assert!(err.contains("unknown kind"), "{err}");

    let proposed = call(
        &client,
        "propose",
        json!({ "node": run.node, "summary": "Split this task.", "changes": { "split": 2 } }),
    )
    .await;
    assert_eq!(proposed["status"], "proposed");
    // A proposal asks a person, without approving anything.
    let item = proposed["attention"].as_str().unwrap().to_owned();
    let raised = served
        .factory
        .attention(Some(root), false)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.r#ref == item)
        .unwrap();
    assert_eq!(
        raised.kind.as_known(),
        Some(wg::AttentionKind::ATTENTION_KIND_PROPOSAL)
    );
    assert_eq!(
        (raised.node.as_str(), raised.run.as_str()),
        (run.node.as_str(), run.r#ref.as_str())
    );
    assert_eq!(raised.summary, "Split this task.");
    let detail: wg::ReconcileNote = raised
        .detail
        .unpack_if(factory_core::schema::RECONCILE_NOTE)
        .unwrap()
        .unwrap();
    assert!(detail.text.contains("\"split\": 2"), "{}", detail.text);
    // Its outcome reaches the agent through the item.
    served
        .factory
        .resolve_attention(
            &item,
            "decline",
            "Keep it whole.",
            &factory_core::Provenance::new("operator", "test"),
        )
        .await
        .unwrap();
    let read = call(&client, "item_read", json!({ "ref": run.node })).await;
    let outcome = read["attention"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["ref"] == item.as_str())
        .unwrap()
        .clone();
    assert_eq!(outcome["resolution"], "decline");
    assert_eq!(outcome["comment"], "Keep it whole.");

    // A reviewer's verdict is an annotation the reconciler reads.
    let verdict = call(
        &client,
        "annotate",
        json!({
            "node": run.node, "kind": "ReviewVerdict",
            "payload": { "run": run.r#ref, "step": 2, "pass": true }
        }),
    )
    .await;
    assert_eq!(verdict["annotation"]["kind"], wg::ReviewVerdict::TYPE_URL);

    // Outside a planning run, submit asks for the approval itself, once.
    let approvals = async || {
        served
            .factory
            .attention(Some(root), false)
            .await
            .unwrap()
            .into_iter()
            .filter(|a| {
                a.node == *root
                    && a.kind.as_known() == Some(wg::AttentionKind::ATTENTION_KIND_APPROVAL)
            })
            .count()
    };
    let before = approvals().await;
    let submitted = call(&client, "submit", json!({ "note": "Ready for review." })).await;
    assert_eq!(submitted["status"], "awaiting_approval");
    assert_eq!(
        submitted["revision"],
        served.factory.graph(root).await.unwrap().revision.as_str()
    );
    assert_eq!(approvals().await, before.max(1));
    call(&client, "submit", json!({})).await;
    assert_eq!(approvals().await, before.max(1), "asked once");
}

/// A planner in a `plan.v1` run leaves the approval to its executor, which
/// asks once the session is idle.
#[tokio::test]
async fn a_planning_run_asks_for_approval_itself() {
    let served = serve().await;
    let mut planning = served.run.clone();
    planning.r#ref = factory_core::olai::new_ref();
    planning.node = served.root.clone();
    planning.executor = factory_core::executor::plan::NAME.into();
    planning.artifacts.clear();
    served
        .factory
        .put_run(
            &planning,
            &factory_core::Provenance::new("reconciler", "test"),
        )
        .await
        .unwrap();
    let grant = Grant::new("factory-planner", ToolPolicy::all())
        .root(served.root.clone())
        .run(planning.r#ref.clone());
    let client = connect(&served, &served.tokens.mint(grant), false).await;
    let submitted = call(&client, "submit", json!({})).await;
    assert_eq!(submitted["status"], "awaiting_approval");
    assert!(submitted["attention"].is_null(), "{submitted}");
}

#[tokio::test]
async fn report_change_records_a_pr_group_in_order() {
    let served = serve().await;
    let run = &served.run;
    let grant = Grant::new("factory-executor", ToolPolicy::names(["report_change"]))
        .root(served.root.clone())
        .run(run.r#ref.clone());
    let client = connect(&served, &served.tokens.mint(grant), false).await;
    let before = run.artifacts.len();

    let prs = |state: &str| {
        json!([
            { "url": "https://example.com/pr/1", "repo": "o/r", "number": 1, "state": state },
            { "url": "https://example.com/pr/2", "repo": "o/r", "number": 2, "state": state },
            { "url": "https://example.com/pr/3", "repo": "o/r", "number": 3, "state": state }
        ])
    };
    let got = call(
        &client,
        "report_change",
        json!({ "group": "stack", "prs": prs("open") }),
    )
    .await;
    assert_eq!(got["run"], run.r#ref.as_str());
    let reported: Vec<&Value> = got["artifacts"].as_array().unwrap()[before..]
        .iter()
        .collect();
    assert_eq!(reported.len(), 3);
    for (i, a) in reported.iter().enumerate() {
        let group = &a["pullRequest"]["group"];
        assert_eq!(group["name"], "stack");
        assert_eq!(group["position"], i + 1);
        assert_eq!(group["of"], 3);
        assert_eq!(a["source"], "ARTIFACT_SOURCE_REPORTED");
    }
    // Reporting again updates by URL.
    let again = call(
        &client,
        "report_change",
        json!({ "group": "stack", "prs": prs("merged") }),
    )
    .await;
    let artifacts = again["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), before + 3);
    assert!(
        artifacts[before..]
            .iter()
            .all(|a| a["pullRequest"]["state"] == "merged")
    );

    // Several PRs without a group name are one group, named after the run.
    let more = json!([
        { "url": "https://example.com/pr/4" },
        { "url": "https://example.com/pr/5" }
    ]);
    let got = call(&client, "report_change", json!({ "prs": more })).await;
    let last = &got["artifacts"].as_array().unwrap()[before + 4]["pullRequest"]["group"];
    assert_eq!(last["name"], run.r#ref.as_str());
    assert_eq!(
        (last["position"].as_u64(), last["of"].as_u64()),
        (Some(2), Some(2))
    );

    // A session reports only for its own run.
    let err = call_err(
        &client,
        "report_change",
        json!({ "run": "other", "prs": prs("open") }),
    )
    .await;
    assert!(err.contains("its own run"), "{err}");
    let stored = served.factory.run(&served.root, &run.r#ref).await.unwrap();
    assert_eq!(stored.artifacts.len(), before + 5);
}

#[tokio::test]
async fn a_session_stays_in_its_plan() {
    let served = serve().await;
    let other = served
        .factory
        .create_node(
            None,
            factory_core::schema::PLAN,
            factory_core::payload::spec("Another plan", "", &wg::Plan::default()),
            Default::default(),
            &factory_core::Provenance::new("operator", "test"),
        )
        .await
        .unwrap()
        .r#ref;
    let grant = Grant::new("factory-executor", ToolPolicy::all()).root(other.clone());
    let client = connect(&served, &served.tokens.mint(grant), false).await;

    let err = call_err(&client, "plan_read", json!({ "root": served.root })).await;
    assert!(err.contains("works on"), "{err}");
    let err = call_err(&client, "item_read", json!({ "ref": served.run.node })).await;
    assert!(err.contains("isn't in the plan"), "{err}");
    let err = call_err(
        &client,
        "add_item",
        json!({ "parent": served.root, "kind": "Task", "title": "sneaky" }),
    )
    .await;
    assert!(err.contains("isn't in the plan"), "{err}");
    let err = call_err(
        &client,
        "annotate",
        json!({ "node": served.run.node, "kind": "reconcile_note", "payload": { "text": "x" } }),
    )
    .await;
    assert!(err.contains("isn't in the plan"), "{err}");

    // Its own plan is fine.
    let plan = call(&client, "plan_read", json!({})).await;
    assert_eq!(plan["root"], other.as_str());
}

#[tokio::test]
async fn an_extension_cant_shadow_a_builtin_tool() {
    let factory = factory(FactoryBuilder::new().extension(Clash)).await;
    let err = WorkgraphMcp::new(factory).unwrap_err();
    assert!(err.to_string().contains("plan_read"), "{err}");
}

/// A client with a grant fixed for the process (stdio), over an in-memory
/// pipe: no token involved.
#[tokio::test]
async fn a_fixed_grant_serves_without_tokens() {
    let factory = factory(FactoryBuilder::new()).await;
    let root = factory_core::demo::seed(&factory).await.unwrap();
    let server = WorkgraphMcp::new(factory)
        .unwrap()
        .with_grant(Grant::new("planner", ToolPolicy::read_only()).root(root.clone()));
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let client = client_config(false).serve(client_io).await.unwrap();
    assert_eq!(
        names(&client).await,
        [
            "item_read",
            "list_profiles",
            "list_repos",
            "plan_read",
            "ready"
        ]
    );
    let plan = call(&client, "plan_read", json!({})).await;
    assert_eq!(plan["root"], root.as_str());
}
