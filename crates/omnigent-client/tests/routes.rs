//! The client's requests, against a fake server: method, path (under a gateway prefix),
//! query and JSON body.

use futures::StreamExt as _;
use omnigent_client::{ElicitationAction, Event, OmnigentClient, api};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

/// A request the fake server received.
#[derive(Debug)]
struct Seen {
    method: String,
    target: String,
    body: Value,
}

/// A server that answers each connection with the next of `answers` (status, content
/// type, body) and reports the request.
async fn serve(
    answers: Vec<(u16, &'static str, String)>,
) -> (url::Url, mpsc::UnboundedReceiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/omnigent", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        for (status, content_type, body) in answers {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            let (head, rest) = loop {
                let n = socket.read(&mut buf).await.unwrap();
                raw.extend_from_slice(&buf[..n]);
                if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    break (
                        String::from_utf8_lossy(&raw[..end]).into_owned(),
                        raw[end + 4..].to_vec(),
                    );
                }
            };
            let length = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse().unwrap())
                })
                .unwrap_or(0usize);
            let mut body_bytes = rest;
            while body_bytes.len() < length {
                let n = socket.read(&mut buf).await.unwrap();
                body_bytes.extend_from_slice(&buf[..n]);
            }
            let mut start = head.lines().next().unwrap().split(' ');
            tx.send(Seen {
                method: start.next().unwrap().to_owned(),
                target: start.next().unwrap().to_owned(),
                body: serde_json::from_slice(&body_bytes).unwrap_or(Value::Null),
            })
            .unwrap();
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
        }
    });
    (base.parse().unwrap(), rx)
}

fn json_answer(body: Value) -> (u16, &'static str, String) {
    (200, "application/json", body.to_string())
}

fn session_json() -> Value {
    json!({"id": "conv_1", "agent_id": "agent_1", "status": "idle", "created_at": 1763251200,
           "kind": "user", "an_unknown_field": {"is": "ignored"}})
}

#[tokio::test]
async fn routes() {
    let ack = json!({"queued": true});
    let (base, mut seen) = serve(vec![
        json_answer(session_json()),
        json_answer(
            json!({"object": "list", "data": [{"id": "conv_1", "agent_id": "agent_1",
            "status": "running", "created_at": 1, "updated_at": 2}], "has_more": false}),
        ),
        json_answer(session_json()),
        json_answer(session_json()),
        json_answer(ack.clone()),
        json_answer(ack.clone()),
        json_answer(ack.clone()),
        json_answer(json!({"queued": false})),
        json_answer(
            json!({"object": "list", "data": [{"id": "agent_1", "name": "lakehouse",
            "created_at": 1, "version": 3}], "has_more": false}),
        ),
        json_answer(
            json!({"hosts": [{"host_id": "host_1", "name": "omnigent-host",
            "status": "online", "configured_harnesses": {"claude-sdk": true, "pi-native": "needs-auth"}}]}),
        ),
    ])
    .await;
    let client = OmnigentClient::new(reqwest::Client::new(), base);

    let session = client
        .create_session("agent_1", |r| {
            r.title = Some("Grant".into());
            r.host_id = Some("host_1".into());
            r.workspace = Some("/workspace".into());
        })
        .await
        .unwrap();
    assert_eq!(session.created_at, 1763251200);
    let request = seen.recv().await.unwrap();
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        ("POST", "/omnigent/v1/sessions")
    );
    assert_eq!(
        request.body,
        json!({"agent_id": "agent_1", "title": "Grant", "host_id": "host_1", "workspace": "/workspace"})
    );

    let list = client
        .sessions()
        .list_sessions(&api::ListSessionsRequest {
            limit: Some(5),
            agent_name: Some("lakehouse".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(list.data[0].updated_at, 2);
    let request = seen.recv().await.unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.target,
        "/omnigent/v1/sessions?limit=5&agent_name=lakehouse"
    );

    client.snapshot("conv 1/x").await.unwrap();
    let request = seen.recv().await.unwrap();
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        (
            "GET",
            "/omnigent/v1/sessions/conv%201%2Fx?include_items=true"
        )
    );

    client
        .sessions()
        .update_session(&api::UpdateSessionRequest {
            session_id: "conv_1".into(),
            title: Some("Renamed".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let request = seen.recv().await.unwrap();
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        ("PATCH", "/omnigent/v1/sessions/conv_1")
    );
    assert_eq!(request.body, json!({"title": "Renamed"}));

    let ack = client.send_message("conv_1", "hello").await.unwrap();
    assert!(ack.queued);
    let request = seen.recv().await.unwrap();
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        ("POST", "/omnigent/v1/sessions/conv_1/events")
    );
    assert_eq!(
        request.body,
        json!({"type": "message", "data": {"role": "user", "content": [{"type": "input_text", "text": "hello"}]}})
    );

    client.interrupt("conv_1").await.unwrap();
    let request = seen.recv().await.unwrap();
    assert_eq!(request.target, "/omnigent/v1/sessions/conv_1/events");
    assert_eq!(request.body["type"], "interrupt");

    client
        .send_function_call_output("conv_1", "call_1", "ok")
        .await
        .unwrap();
    let request = seen.recv().await.unwrap();
    assert_eq!(
        request.body,
        json!({"type": "function_call_output", "data": {"call_id": "call_1", "output": "ok"}})
    );

    let content = json!({"privileges": ["SELECT"], "count": 2});
    client
        .resolve_elicitation(
            "conv_1",
            "elicit_1",
            ElicitationAction::Accept,
            content.as_object().cloned(),
        )
        .await
        .unwrap();
    let request = seen.recv().await.unwrap();
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        (
            "POST",
            "/omnigent/v1/sessions/conv_1/elicitations/elicit_1/resolve"
        )
    );
    // Whole numbers stay integers through the Struct.
    assert_eq!(
        request.body,
        json!({"action": "accept", "content": content})
    );

    let agents = client
        .agents()
        .list_agents(&Default::default())
        .await
        .unwrap();
    assert_eq!(agents.data[0].name, "lakehouse");
    assert_eq!(seen.recv().await.unwrap().target, "/omnigent/v1/agents");

    let hosts = client
        .hosts()
        .list_hosts(&Default::default())
        .await
        .unwrap();
    assert_eq!(hosts.hosts[0].status, "online");
    assert_eq!(seen.recv().await.unwrap().target, "/omnigent/v1/hosts");
}

#[tokio::test]
async fn errors() {
    let (base, _seen) = serve(vec![
        (
            404,
            "application/json",
            json!({"error": {"code": "not_found", "message": "No session"}}).to_string(),
        ),
        (
            422,
            "application/json",
            json!({"detail": "agent_id is required"}).to_string(),
        ),
    ])
    .await;
    let client = OmnigentClient::new(reqwest::Client::new(), base);
    let error = client.snapshot("nope").await.unwrap_err();
    assert!(error.is_not_found());
    assert_eq!(error.code(), Some("not_found"));
    assert_eq!(error.to_string(), "not_found: No session");
    let error = client.send_message("x", "hi").await.unwrap_err();
    assert_eq!(error.status(), Some(422));
    assert_eq!(error.to_string(), "HTTP 422: agent_id is required");
}

#[tokio::test]
async fn stream() {
    let (base, mut seen) = serve(vec![
        (
            200,
            "text/event-stream",
            include_str!("../fixtures/grant-turn.sse").to_owned(),
        ),
        (
            404,
            "application/json",
            json!({"error": {"code": "not_found", "message": "gone"}}).to_string(),
        ),
    ])
    .await;
    let client = OmnigentClient::new(reqwest::Client::new(), base);
    let events: Vec<Event> = client
        .stream("conv_1")
        .await
        .unwrap()
        .map(|e| e.unwrap())
        .collect()
        .await;
    assert_eq!(events.len(), 28, "the recorded turn's frames");
    let request = seen.recv().await.unwrap();
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        ("GET", "/omnigent/v1/sessions/conv_1/stream")
    );
    assert!(client.stream("conv_1").await.unwrap_err().is_not_found());
}

/// What the factory's sessions use: the create fields (labels, git, sandbox, overrides),
/// finding sessions by label, the pull requests, and the snapshot's cost and error.
#[tokio::test]
async fn factory_sessions() {
    let item = |id: &str, node: &str| {
        json!({"id": id, "agent_id": "agent_1", "status": "idle", "created_at": 1,
               "updated_at": 2, "labels": {"factory.plan": "p1", "factory.node": node}})
    };
    let pr = |number: i64, relationship: &str| {
        json!({"url": format!("https://github.com/o/r/pull/{number}"), "provider": "github",
               "repository": "o/r", "number": number, "host": "github.com", "state": "OPEN",
               "is_draft": false, "relationship": relationship, "first_seen_at": 1.5,
               "last_seen_at": 2.5, "provider_display": null})
    };
    let (base, mut seen) = serve(vec![
        json_answer(session_json()),
        json_answer(
            json!({"object": "list", "has_more": true, "last_id": "conv_2",
            "data": [item("conv_1", "t1"), item("conv_2", "t2")]}),
        ),
        json_answer(
            json!({"object": "list", "has_more": false, "last_id": "conv_3",
            "data": [{"id": "conv_3", "agent_id": "agent_1", "status": "running",
                      "created_at": 1, "updated_at": 2,
                      "labels": {"factory.plan": "p1", "factory.node": "t1"},
                      "git_branch": "factory/t1", "pull_requests_synced": true,
                      "pull_request": {"url": "https://github.com/o/r/pull/7",
                          "provider": "github", "repository": "o/r", "number": 7,
                          "state": "MERGED", "is_draft": false}}]}),
        ),
        json_answer(
            json!({"object": "list", "data": [pr(7, "created")], "has_more": true,
            "last_id": "https://github.com/o/r/pull/7", "synced": true}),
        ),
        json_answer(
            json!({"object": "list", "data": [pr(8, "inferred")], "has_more": false,
            "synced": true}),
        ),
        json_answer(
            json!({"id": "conv_3", "agent_id": "agent_1", "status": "failed",
            "created_at": 1, "total_cost_usd": 0.42, "harness": "claude-sdk",
            "last_task_error": {"code": "executor_error", "message": "boom"}}),
        ),
        json_answer(
            json!({"id": "conv_3", "agent_id": "agent_1", "status": "idle",
            "created_at": 1, "total_cost_usd": null, "last_task_error": null}),
        ),
    ])
    .await;
    let client = OmnigentClient::new(reqwest::Client::new(), base);

    client
        .create_session("agent_1", |r| {
            r.labels.insert("factory.node".into(), "t1".into());
            r.host_type = Some("managed".into());
            r.sandbox_provider = Some("docker".into());
            r.sandbox_profile_id = Some("prof_1".into());
            r.git = api::GitOptions {
                branch_name: "factory/t1".into(),
                base_branch: Some("main".into()),
                ..Default::default()
            }
            .into();
            r.model_override = Some("claude-opus-5-5".into());
            r.harness_override = Some("codex".into());
            r.reasoning_effort = Some("high".into());
        })
        .await
        .unwrap();
    let request = seen.recv().await.unwrap();
    assert_eq!(
        request.body,
        json!({"agent_id": "agent_1", "labels": {"factory.node": "t1"}, "host_type": "managed",
               "sandbox_provider": "docker", "sandbox_profile_id": "prof_1",
               "git": {"branch_name": "factory/t1", "base_branch": "main"},
               "model_override": "claude-opus-5-5", "harness_override": "codex",
               "reasoning_effort": "high"})
    );

    let wanted = [("factory.plan", "p1"), ("factory.node", "t1")]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    let found = client.find_sessions(&wanted, false).await.unwrap();
    let ids: Vec<_> = found.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["conv_1", "conv_3"], "conv_2 is another node's");
    let summary = found[1].pull_request.as_option().unwrap();
    assert_eq!(
        (summary.number, summary.state.as_deref()),
        (7, Some("MERGED"))
    );
    assert_eq!(found[1].git_branch.as_deref(), Some("factory/t1"));
    assert_eq!(
        seen.recv().await.unwrap().target,
        "/omnigent/v1/sessions?limit=1000&include_archived=false&kind=any"
    );
    assert_eq!(
        seen.recv().await.unwrap().target,
        "/omnigent/v1/sessions?limit=1000&after=conv_2&include_archived=false&kind=any"
    );

    let (prs, synced) = client.pull_requests("conv_3").await.unwrap();
    assert!(synced);
    let got: Vec<_> = prs
        .iter()
        .map(|p| (p.number, p.relationship.as_str(), p.state.as_deref()))
        .collect();
    assert_eq!(
        got,
        [(7, "created", Some("OPEN")), (8, "inferred", Some("OPEN"))]
    );
    assert_eq!(prs[0].first_seen_at, 1.5);
    assert_eq!(
        seen.recv().await.unwrap().target,
        "/omnigent/v1/sessions/conv_3/pull-requests?limit=100"
    );
    assert_eq!(
        seen.recv().await.unwrap().target,
        "/omnigent/v1/sessions/conv_3/pull-requests?limit=100&after=https%3A%2F%2Fgithub.com%2Fo%2Fr%2Fpull%2F7"
    );

    let failed = client.snapshot("conv_3").await.unwrap();
    assert_eq!(failed.total_cost_usd, Some(0.42));
    assert_eq!(failed.harness.as_deref(), Some("claude-sdk"));
    assert_eq!(
        failed.last_task_error.get("code").map(String::as_str),
        Some("executor_error")
    );
    let idle = client.snapshot("conv_3").await.unwrap();
    assert_eq!(idle.total_cost_usd, None, "unpriced");
    assert!(idle.last_task_error.is_empty());
}
