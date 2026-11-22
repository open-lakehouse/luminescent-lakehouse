//! workgraph-mcp end to end: the demo plan on the in-memory backend, served
//! over Streamable HTTP behind bearer tokens, and driven by rmcp's client,
//! as an agent (and the desktop's side channel) would.

use std::sync::Arc;

use async_trait::async_trait;
use factory_core::tool::{
    MCP_APP_MIME_TYPE, ToolCall, ToolHandler, ToolOutput, ToolSpec, UiResource,
};
use factory_core::{Extension, Factory, FactoryBuilder, condition, olai};
use factory_proto::workgraph::v1 as wg;
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, ContentBlock,
    ExtensionCapabilities, Implementation, ReadResourceRequestParams, ResourceContents,
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
    fn ui_resources(&self) -> Vec<UiResource> {
        vec![
            UiResource::new("ui://audit/view", "audit", "<!doctype html><p>audit</p>")
                .prefers_border(false),
        ]
    }
}

/// An extension whose view takes plan review's URI.
struct ViewClash;
impl Extension for ViewClash {
    fn name(&self) -> &'static str {
        "view-clash"
    }
    fn ui_resources(&self) -> Vec<UiResource> {
        vec![UiResource::new(
            workgraph_mcp::review::PLAN_VIEW,
            "mine",
            "",
        )]
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
    serve_with(None).await
}

/// The demo, served with `control_room` as the control room's URL.
async fn serve_with(control_room: Option<&str>) -> Served {
    let factory = factory(FactoryBuilder::new().extension(Audit)).await;
    let root = factory_core::demo::seed(&factory).await.unwrap();
    let run = factory.runs(&root).await.unwrap().remove(0);
    let tokens = Tokens::new();
    let mut server = WorkgraphMcp::new(factory.clone()).unwrap();
    if let Some(url) = control_room {
        server = server.with_control_room(url);
    }
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

/// A successful call's text and structured result.
async fn call_text(client: &Client, name: &str, args: Value) -> (String, Value) {
    let result = try_call(client, name, args).await.unwrap();
    assert_ne!(result.is_error, Some(true), "{name}: {:?}", result.content);
    let text = result
        .content
        .iter()
        .filter_map(ContentBlock::as_text)
        .map(|t| t.text.clone())
        .collect();
    (text, result.structured_content.expect("structured content"))
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

/// The built-in tools every policy-allowed client sees.
const BUILTIN: [&str; 12] = [
    "add_edge",
    "add_item",
    "annotate",
    "item_read",
    "list_profiles",
    "list_repos",
    "open_plan_review",
    "plan_read",
    "propose",
    "ready",
    "report_change",
    "submit",
];

/// The built-in app-only tools: an MCP Apps client with an apps grant only.
const APP_ONLY: [&str; 2] = ["approve_revision", "return_plan"];

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
    // `apps`: the client declares MCP Apps; `host`: the grant is an apps
    // host's (the desktop's), not an agent's.
    let list = async |policy: &str, apps: bool, host: bool| {
        let mut grant = Grant::new("someone", policy.parse().unwrap()).role("whatever");
        if host {
            grant = grant.apps();
        }
        let token = served.tokens.mint(grant);
        names(&connect(&served, &token, apps).await).await
    };

    let mut everything: Vec<String> = BUILTIN.iter().map(|s| (*s).to_owned()).collect();
    everything.push("audit_whoami".into());
    everything.sort();
    assert_eq!(list("*", false, false).await, everything);

    // App-only tools exist only for an apps host's grant on a client that
    // declares MCP Apps: an agent's token never sees them, whatever its
    // client declares.
    assert_eq!(list("*", true, false).await, everything);
    assert_eq!(list("*", false, true).await, everything);
    let mut with_app = everything.clone();
    with_app.push("audit_approve".into());
    with_app.extend(APP_ONLY.map(String::from));
    with_app.sort();
    assert_eq!(list("*", true, true).await, with_app);

    assert_eq!(
        list("@read", true, true).await,
        [
            "audit_whoami",
            "item_read",
            "list_profiles",
            "list_repos",
            "open_plan_review",
            "plan_read",
            "ready"
        ]
    );
    assert_eq!(
        list("plan_read,ready,report_change", false, false).await,
        ["plan_read", "ready", "report_change"]
    );
    assert_eq!(
        list("audit_*", true, true).await,
        ["audit_approve", "audit_whoami"]
    );
    assert_eq!(list("audit_*", true, false).await, ["audit_whoami"]);
    assert_eq!(
        list("@write,!add_*,!audit_*", true, true).await,
        [
            "annotate",
            "approve_revision",
            "propose",
            "report_change",
            "return_plan",
            "submit"
        ]
    );
    assert!(
        list("", false, false).await.is_empty(),
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
        .mint(Grant::new("operator", ToolPolicy::all()).apps());
    let client = connect(&served, &token, true).await;
    let tools = client.list_all_tools().await.unwrap();
    let review = tools.iter().find(|t| t.name == "open_plan_review").unwrap();
    let meta = serde_json::to_value(review.meta.as_ref().unwrap()).unwrap();
    assert_eq!(
        meta,
        json!({ "ui": { "resourceUri": "ui://factory/plan" } })
    );
    assert_eq!(
        review.annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );
    for name in APP_ONLY {
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        let meta = serde_json::to_value(tool.meta.as_ref().unwrap()).unwrap();
        assert_eq!(meta, json!({ "ui": { "visibility": ["app"] } }), "{name}");
    }
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
            "open_plan_review",
            "plan_read",
            "ready"
        ]
    );
    let plan = call(&client, "plan_read", json!({})).await;
    assert_eq!(plan["root"], root.as_str());
}

// --- plan review (MCP App) --------------------------------------------------

/// An apps host's client: the desktop's side channel.
async fn desktop(served: &Served) -> Client {
    let token = served
        .tokens
        .mint(Grant::new("person@example.com", ToolPolicy::all()).apps());
    connect(served, &token, true).await
}

async fn read_view(client: &Client, uri: &str) -> Result<ResourceContents, rmcp::ServiceError> {
    let mut read = client
        .read_resource(ReadResourceRequestParams::new(uri))
        .await?;
    Ok(read.contents.remove(0))
}

#[tokio::test]
async fn views_are_served_to_apps_hosts_only() {
    let served = serve().await;
    let host = desktop(&served).await;
    let uris: Vec<String> = host
        .list_all_resources()
        .await
        .unwrap()
        .into_iter()
        .map(|r| {
            assert_eq!(r.mime_type.as_deref(), Some(MCP_APP_MIME_TYPE));
            r.uri.clone()
        })
        .collect();
    assert_eq!(uris, ["ui://factory/plan", "ui://audit/view"]);

    let ResourceContents::TextResourceContents {
        mime_type, text, ..
    } = read_view(&host, "ui://factory/plan").await.unwrap()
    else {
        panic!("a view is text");
    };
    assert_eq!(mime_type.as_deref(), Some(MCP_APP_MIME_TYPE));
    assert_eq!(text, workgraph_mcp::review::PLAN_VIEW_HTML);
    let ResourceContents::TextResourceContents { meta, .. } =
        read_view(&host, "ui://audit/view").await.unwrap()
    else {
        panic!("a view is text");
    };
    let meta = serde_json::to_value(meta.unwrap()).unwrap();
    assert_eq!(meta, json!({ "ui": { "prefersBorder": false } }));

    // An agent's token sees no views, even on a client declaring MCP Apps,
    // and neither does an apps grant on a client that doesn't.
    let agent = served.tokens.mint(Grant::new("planner", ToolPolicy::all()));
    let host_grant = served
        .tokens
        .mint(Grant::new("person", ToolPolicy::all()).apps());
    for client in [
        connect(&served, &agent, true).await,
        connect(&served, &host_grant, false).await,
    ] {
        assert!(client.list_all_resources().await.unwrap().is_empty());
        let err = read_view(&client, "ui://factory/plan").await.unwrap_err();
        assert!(err.to_string().contains("no resource"), "{err}");
    }
}

#[tokio::test]
async fn an_extensions_view_cant_take_a_builtin_uri() {
    let factory = factory(FactoryBuilder::new().extension(ViewClash)).await;
    let err = WorkgraphMcp::new(factory).unwrap_err();
    assert!(err.to_string().contains("ui://factory/plan"), "{err}");
}

fn decode<M: buffa::Message + Default>(b64: &Value) -> M {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_str().unwrap())
        .unwrap();
    M::decode_from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn a_plan_review_shows_the_drift_and_approves_what_was_reviewed() {
    use factory_proto::v1;
    let served = serve().await;
    let root = served.root.as_str();
    let planner = served.tokens.mint(
        Grant::new("factory-planner", ToolPolicy::all())
            .role("planner")
            .root(root),
    );
    let agent = connect(&served, &planner, false).await;
    let submitted = call(&agent, "submit", json!({ "note": "Ready." })).await;
    let revision = submitted["revision"].as_str().unwrap().to_owned();

    // The agent opens the review: text for the model, the snapshot for the view.
    let result = try_call(&agent, "open_plan_review", json!({}))
        .await
        .unwrap();
    let text = result.content[0].as_text().unwrap().text.clone();
    assert!(text.contains("Nothing is approved"), "{text}");
    let review = result.structured_content.unwrap();
    assert_eq!(review["status"], "awaiting_user");
    assert_eq!(review["revision"], revision.as_str());
    let approved = review["approved_revision"].as_str().unwrap().to_owned();
    assert_ne!(approved, revision, "the demo drifted from its approval");
    assert!(review["changes"]["added"].as_u64().unwrap() > 0, "{review}");

    let snapshot = &review["snapshot"];
    let graph: v1::GetGraphResponse = decode(&snapshot["graph"]);
    assert_eq!(graph.revision, revision);
    assert_eq!(graph.approved_revision, approved);
    let target: v1::GetRevisionResponse = decode(&snapshot["target"]);
    assert_eq!(target.revision.root_hash, revision);
    let base: v1::GetRevisionResponse = decode(&snapshot["base"]);
    assert_eq!(base.revision.root_hash, approved);
    assert!(base.revision.approved);
    let diff: v1::DiffRevisionsResponse = decode(&snapshot["diff"]);
    let full: v1::DiffRevisionsResponse = decode(&snapshot["diff_full"]);
    assert!(!diff.entries.is_empty() && full.entries.len() >= diff.entries.len());
    let schema: v1::GetSchemaResponse = decode(&snapshot["schema"]);
    assert!(!schema.schema.node_kinds.is_empty());

    // The agent can't approve: the tools don't exist for it.
    let err = try_call(&agent, "approve_revision", json!({ "revision": revision }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("unknown tool"), "{err}");

    // The person approves from the view, at the revision they reviewed.
    let host = desktop(&served).await;
    let stale = call_err(
        &host,
        "approve_revision",
        json!({ "root": root, "revision": approved }),
    )
    .await;
    assert!(stale.contains("isn't the current revision"), "{stale}");
    let done = call(
        &host,
        "approve_revision",
        json!({ "root": root, "revision": revision }),
    )
    .await;
    assert_eq!(done["status"], "approved");
    let plan = call(&host, "plan_read", json!({ "root": root })).await;
    assert_eq!(plan["approved_revision"], revision.as_str());
    assert!(
        plan["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["phase"] != "proposed"),
        "{plan}"
    );
    let approvals: Vec<wg::Attention> = served
        .factory
        .attention(Some(root), true)
        .await
        .unwrap()
        .into_iter()
        .filter(|a| a.kind.as_known() == Some(wg::AttentionKind::ATTENTION_KIND_APPROVAL))
        .collect();
    assert!(!approvals.is_empty());
    assert!(
        approvals.iter().all(|a| a.resolved
            && a.resolution == "accept"
            && a.resolved_by == "person@example.com")
    );
}

#[tokio::test]
async fn a_returned_plan_carries_the_notes() {
    let served = serve().await;
    let root = served.root.as_str();
    let host = desktop(&served).await;
    call(&host, "submit", json!({ "root": root })).await;

    let missing = call_err(&host, "return_plan", json!({ "root": root })).await;
    assert!(missing.contains("notes"), "{missing}");
    let returned = call(
        &host,
        "return_plan",
        json!({ "root": root, "notes": "Split the server task." }),
    )
    .await;
    assert_eq!(returned["status"], "returned");

    let approval = served
        .factory
        .attention(Some(root), true)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.kind.as_known() == Some(wg::AttentionKind::ATTENTION_KIND_APPROVAL))
        .unwrap();
    assert!(approval.resolved);
    assert_eq!(approval.resolution, "decline");
    assert_eq!(approval.comment, "Split the server task.");
    let notes = served.factory.annotations(root).await.unwrap();
    let note = notes
        .iter()
        .filter_map(|a| {
            a.payload
                .as_option()?
                .unpack_unchecked::<wg::ReconcileNote>()
                .ok()
        })
        .find(|n| n.text.contains("Returned at revision"))
        .expect("the notes are on the plan");
    assert!(
        note.text.ends_with("Split the server task."),
        "{}",
        note.text
    );
    let plan = call(&host, "plan_read", json!({ "root": root })).await;
    assert_ne!(
        plan["approved_revision"], plan["revision"],
        "nothing approved"
    );
}

/// `open_plan_review`'s result on the demo plan, as the desktop's e2e fakes
/// serve it to the real view (`apps/desktop/e2e/fixtures/open_plan_review.json`).
/// Revisions carry times, so the fixture can't match byte for byte: it is
/// rewritten with `UPDATE_FIXTURES=1`, and otherwise checked for the same
/// shape and a snapshot that still decodes.
#[tokio::test]
async fn the_desktop_fixture_is_current() {
    use factory_proto::v1;
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop/e2e/fixtures/open_plan_review.json"
    );
    let served = serve().await;
    let host = desktop(&served).await;
    let result = try_call(&host, "open_plan_review", json!({ "root": served.root }))
        .await
        .unwrap();
    let current = serde_json::to_value(&result).unwrap();
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        let text = serde_json::to_string_pretty(&current).unwrap() + "\n";
        std::fs::write(path, text).unwrap();
        return;
    }
    let committed: Value = serde_json::from_str(
        &std::fs::read_to_string(path).expect("UPDATE_FIXTURES=1 writes the fixture"),
    )
    .unwrap();
    let keys = |v: &Value| -> Vec<String> {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    let (now, then) = (
        &current["structuredContent"],
        &committed["structuredContent"],
    );
    assert_eq!(keys(now), keys(then), "rerun with UPDATE_FIXTURES=1");
    assert_eq!(
        keys(&now["snapshot"]),
        keys(&then["snapshot"]),
        "rerun with UPDATE_FIXTURES=1"
    );
    assert_eq!(then["status"], "awaiting_user");
    let graph: v1::GetGraphResponse = decode(&then["snapshot"]["graph"]);
    assert_eq!(graph.revision, then["revision"].as_str().unwrap());
    let _: v1::GetRevisionResponse = decode(&then["snapshot"]["target"]);
    let _: v1::DiffRevisionsResponse = decode(&then["snapshot"]["diff"]);
}

#[tokio::test]
async fn agents_get_markdown_and_the_json_stays_structured() {
    let served = serve_with(Some("http://cr.test")).await;
    let root = served.root.as_str();
    let planner = served.tokens.mint(
        Grant::new("factory-planner", ToolPolicy::all())
            .role("planner")
            .root(root),
    );
    let agent = connect(&served, &planner, false).await;

    // The whole plan: a header, one graph, a row per item with its ref.
    let (text, plan) = call_text(&agent, "plan_read", json!({})).await;
    assert!(text.starts_with("## Plan: "), "{text}");
    assert_eq!(text.matches("```mermaid").count(), 1, "{text}");
    let items = plan["items"].as_array().unwrap();
    assert!(items.len() > 3, "{plan}");
    for item in items.iter().filter(|i| i["ref"] != root) {
        let r = item["ref"].as_str().unwrap();
        assert!(text.contains(&format!("`{r}`")), "{r} missing:\n{text}");
    }
    assert_eq!(
        plan["revision"].as_str().map(|r| text.contains(r)),
        Some(true)
    );
    assert!(plan.get("under").is_none(), "{plan}");

    // A subtree: the item, what's below it, and the edges touching them.
    let task = items
        .iter()
        .find(|i| i["kind"] == "Task")
        .and_then(|i| i["ref"].as_str())
        .unwrap()
        .to_owned();
    let (text, part) = call_text(&agent, "plan_read", json!({ "under": task })).await;
    assert!(text.contains("(part of the plan"), "{text}");
    assert_eq!(part["under"], task.as_str());
    assert!(
        part["items"].as_array().unwrap().len() < items.len(),
        "{part}"
    );
    let err = call_err(&agent, "plan_read", json!({ "under": "olai:nope" })).await;
    assert!(err.contains("olai:nope"), "{err}");

    let (text, _) = call_text(&agent, "ready", json!({})).await;
    assert!(text.starts_with("## Ready: "), "{text}");
    let (text, item) = call_text(&agent, "item_read", json!({ "ref": task })).await;
    assert!(text.starts_with("## Task "), "{text}");
    assert_eq!(item["item"]["ref"], task.as_str());

    // Writes: the revision, the diff, and the control room to approve in.
    let (text, submitted) = call_text(&agent, "submit", json!({})).await;
    let revision = submitted["revision"].as_str().unwrap();
    assert!(text.contains(revision), "{text}");
    assert!(
        text.contains("Changes since the approved revision"),
        "{text}"
    );
    let link = format!("http://cr.test/?plan={}", root.replace(':', "%3A"));
    assert!(text.contains(&link), "{text}");
    let (text, review) = call_text(&agent, "open_plan_review", json!({})).await;
    assert!(text.contains("can't be shown in this client"), "{text}");
    assert!(text.contains(&link), "{text}");
    assert_eq!(review["status"], "awaiting_user");
    let (text, proposal) = call_text(
        &agent,
        "propose",
        json!({ "node": task, "summary": "Split it." }),
    )
    .await;
    assert!(
        text.contains(proposal["attention"].as_str().unwrap()) && text.contains(&link),
        "{text}"
    );

    // An MCP Apps host gets plan review's wording, not the link.
    let desktop = served
        .tokens
        .mint(Grant::new("operator", ToolPolicy::all()).apps());
    let host = connect(&served, &desktop, true).await;
    let (text, _) = call_text(&host, "open_plan_review", json!({ "root": root })).await;
    assert!(
        text.contains("Nothing is approved until the person decides"),
        "{text}"
    );
    assert!(!text.contains("http://cr.test"), "{text}");
}

#[tokio::test]
async fn without_a_control_room_the_text_says_where_to_approve() {
    let served = serve().await;
    let planner = served.tokens.mint(
        Grant::new("factory-planner", ToolPolicy::all())
            .role("planner")
            .root(served.root.as_str()),
    );
    let agent = connect(&served, &planner, false).await;
    let (text, _) = call_text(&agent, "submit", json!({})).await;
    assert!(text.contains("in the factory's control room"), "{text}");
    assert!(!text.contains("http"), "{text}");
}

// The hosted router: stateless, POST only, tokens from configuration.

/// The demo behind `http::hosted_router`, with a planner role token, a
/// reviewer one given as its hash, and an apps token.
async fn serve_hosted() -> (String, String) {
    let factory = factory(FactoryBuilder::new()).await;
    let root = factory_core::demo::seed(&factory).await.unwrap();
    let reviewer = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(b"wg_reviewer"))
    };
    let tokens = Tokens::from_config(
        &[
            "planner:wg_planner=@read,add_item".into(),
            format!("reviewer:sha256:{reviewer}=@read,annotate"),
        ],
        &["wg_desk=*".into()],
    )
    .unwrap();
    let server = WorkgraphMcp::new(factory).unwrap();
    let router = http::hosted_router(server, tokens, http::PATH);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await });
    (format!("http://{addr}{}", http::PATH), root)
}

/// A client of `url` on `version`: a 2025-11-25 one by `initialize`, a
/// 2026-07-28 one by `server/discover`, with its capabilities in every
/// request's `_meta`. Neither has a session.
async fn connect_hosted(
    url: &str,
    token: &str,
    apps: bool,
    version: rmcp::model::ProtocolVersion,
) -> Client {
    use rmcp::service::{ClientLifecycleMode, ClientServiceExt};

    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url).auth_header(token),
    );
    let config = client_config(apps);
    if version.has_initialize() {
        config
            .with_protocol_version(version)
            .serve(transport)
            .await
            .unwrap()
    } else {
        let lifecycle = ClientLifecycleMode::Discover {
            preferred_versions: vec![version],
        };
        config
            .serve_with_lifecycle(transport, lifecycle)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn the_hosted_router_is_post_only_and_token_first() {
    let (url, _) = serve_hosted().await;
    let http = reqwest::Client::new();
    for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
        let res = http.request(method.clone(), &url).send().await.unwrap();
        assert_eq!(res.status(), 405, "{method}");
        assert_eq!(res.headers()["allow"], "POST");
    }
    let res = http
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn the_hosted_router_serves_both_protocol_versions_without_sessions() {
    use rmcp::model::ProtocolVersion;

    let (url, root) = serve_hosted().await;
    for version in [ProtocolVersion::V_2025_11_25, ProtocolVersion::LATEST] {
        let planner = connect_hosted(&url, "wg_planner", false, version.clone()).await;
        let mut seen = names(&planner).await;
        seen.sort();
        assert!(seen.contains(&"add_item".to_owned()), "{version:?}");
        assert!(!seen.contains(&"annotate".to_owned()), "{version:?}");
        let plan = call(&planner, "plan_read", json!({ "root": root })).await;
        assert!(plan["items"].as_array().unwrap().len() > 1, "{version:?}");

        // The hashed entry stands for its token.
        let reviewer = connect_hosted(&url, "wg_reviewer", false, version.clone()).await;
        let seen = names(&reviewer).await;
        assert!(seen.contains(&"annotate".to_owned()) && !seen.contains(&"add_item".to_owned()));
    }

    // On 2026-07-28 the capabilities travel with each request, so the apps
    // token gets the app-only tools; an agent's token doesn't, declared or not.
    let desk = connect_hosted(&url, "wg_desk", true, ProtocolVersion::LATEST).await;
    let seen = names(&desk).await;
    for tool in APP_ONLY {
        assert!(seen.contains(&tool.to_owned()), "{tool}");
    }
    let agent = connect_hosted(&url, "wg_planner", true, ProtocolVersion::LATEST).await;
    let seen = names(&agent).await;
    for tool in APP_ONLY {
        assert!(!seen.contains(&tool.to_owned()), "{tool}");
    }

    // A 2025-11-25 client's `initialize` declaration doesn't survive to its
    // next request: the desktop's connection declares it per request instead.
    let legacy = connect_hosted(&url, "wg_desk", true, ProtocolVersion::V_2025_11_25).await;
    assert!(
        !names(&legacy)
            .await
            .contains(&"approve_revision".to_owned())
    );
}
