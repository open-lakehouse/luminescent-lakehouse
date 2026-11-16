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
