//! The fold of events into messages, over the grant-turn fixture.

use omnigent_client::sse::{Frame, SseDecoder};
use omnigent_client::{
    Approval, Conversation, ElicitationRequest, Event, Message, OutputItem, Part, Role, ToolCall,
    ToolName, ToolState, api,
};
use serde_json::json;

const GRANT_TURN: &str = include_str!("../fixtures/grant-turn.sse");
const GRANT_SESSION: &str = include_str!("../fixtures/grant-turn.session.json");

fn events(body: &str) -> Vec<Event> {
    let mut decoder = SseDecoder::new();
    let mut frames = decoder.push(body.as_bytes());
    frames.extend(decoder.finish());
    frames
        .into_iter()
        .filter_map(|f| match f {
            Frame::Event { event, data } => Some(Event::parse(event.as_deref(), &data)),
            Frame::Done => None,
        })
        .collect()
}

fn fold(events: &[Event]) -> Conversation {
    let mut conversation = Conversation::default();
    for event in events {
        conversation.apply(event);
    }
    conversation
}

fn text(text: &str) -> Part {
    Part::Text {
        text: text.into(),
        streaming: false,
    }
}

fn grant_turn() -> Vec<Message> {
    vec![
        Message {
            id: "msg_user_1".into(),
            role: Role::User,
            parts: vec![text("grant SELECT on main.default.t to group x")],
        },
        Message {
            id: "resp_1".into(),
            role: Role::Assistant,
            parts: vec![
                Part::Reasoning {
                    text: "The grant form collects the privileges.".into(),
                    streaming: false,
                },
                text("I'll open the grant form."),
                Part::Tool(ToolCall {
                    call_id: "call_1".into(),
                    name: "uc__open_grant_form".into(),
                    input: json!({
                        "securable_type": "table",
                        "full_name": "main.default.t",
                        "principal": "x",
                        "privileges": ["SELECT"],
                    }),
                    arguments: r#"{"securable_type": "table", "full_name": "main.default.t", "principal": "x", "privileges": ["SELECT"]}"#.into(),
                    state: ToolState::OutputAvailable,
                    output: Some("Opened the grant form for main.default.t (SELECT to x).".into()),
                    error: None,
                    approval: None,
                }),
                text("The form is open: check it and submit. ✅"),
            ],
        },
    ]
}

fn session() -> api::Session {
    serde_json::from_str(GRANT_SESSION).expect("the snapshot fixture")
}

#[test]
fn folds_the_stream() {
    let conversation = fold(&events(GRANT_TURN));
    assert_eq!(conversation.messages(), grant_turn());
    assert_eq!(conversation.status(), Some("idle"));
    assert!(conversation.elicitations().is_empty());
}

#[test]
fn folds_the_snapshot_the_same() {
    let conversation = Conversation::from_session(&session());
    assert_eq!(conversation.messages(), grant_turn());
}

#[test]
fn a_replay_over_the_snapshot_changes_nothing() {
    // A reconnect: the stream opened before the snapshot was read, so it repeats it.
    let mut conversation = Conversation::from_session(&session());
    for event in events(GRANT_TURN) {
        conversation.apply(&event);
    }
    assert_eq!(conversation.messages(), grant_turn());
}

#[test]
fn reports_what_changed() {
    let events = events(GRANT_TURN);
    let mut conversation = Conversation::default();
    let changed: Vec<Vec<String>> = events.iter().map(|e| conversation.apply(e)).collect();
    assert_eq!(changed[0], ["msg_user_1"]);
    assert!(changed[1].is_empty(), "a status changes no message");
    assert_eq!(
        changed[2],
        ["resp_1"],
        "response.created starts the message"
    );
    assert!(changed.iter().skip(3).flatten().all(|id| id == "resp_1"));
}

#[test]
fn streams_text_then_replaces_it_with_the_final() {
    let mut conversation = Conversation::default();
    conversation.apply(&Event::parse(
        Some("response.created"),
        r#"{"response":{"id":"r"}}"#,
    ));
    conversation.apply(&Event::OutputTextDelta {
        delta: "Hi th".into(),
    });
    conversation.apply(&Event::OutputTextDelta {
        delta: "ere".into(),
    });
    assert_eq!(
        conversation.messages()[0].parts,
        [Part::Text {
            text: "Hi there".into(),
            streaming: true
        }]
    );
    conversation.apply(&Event::OutputItemDone(OutputItem::from_stream(
        json!({"id": "m", "type": "message", "response_id": "r", "role": "assistant",
               "content": [{"type": "output_text", "text": "Hi there!"}]})
        .as_object()
        .unwrap(),
    )));
    assert_eq!(conversation.messages()[0].parts, [text("Hi there!")]);
}

fn call_item(call_id: &str, status: &str) -> Event {
    Event::OutputItemDone(OutputItem::from_stream(
        json!({"id": format!("fc_{call_id}"), "type": "function_call", "status": status,
               "response_id": "r", "name": "uc__apply_grants", "arguments": "{}",
               "call_id": call_id})
        .as_object()
        .unwrap(),
    ))
}

fn output_item(call_id: &str, status: &str, output: &str) -> Event {
    Event::OutputItemDone(OutputItem::from_stream(
        json!({"id": format!("fco_{call_id}"), "type": "function_call_output",
               "status": status, "response_id": "r", "call_id": call_id, "output": output})
        .as_object()
        .unwrap(),
    ))
}

fn approval_request(id: &str) -> Event {
    Event::ElicitationRequest(ElicitationRequest::from_json(
        json!({"elicitation_id": id, "params": {"message": "Run it?", "phase": "pre_tool_use"}})
            .as_object()
            .unwrap(),
    ))
}

fn tool(conversation: &Conversation) -> &ToolCall {
    conversation
        .messages()
        .iter()
        .flat_map(|m| &m.parts)
        .find_map(|p| match p {
            Part::Tool(call) => Some(call),
            _ => None,
        })
        .expect("a tool call")
}

#[test]
fn tool_states() {
    // Approved, then run.
    let mut conversation = fold(&[call_item("c", "in_progress"), approval_request("e")]);
    assert_eq!(tool(&conversation).state, ToolState::ApprovalRequested);
    assert_eq!(
        tool(&conversation).approval,
        Some(Approval {
            id: "e".into(),
            approved: None,
            reason: None
        })
    );
    assert_eq!(conversation.elicitations().len(), 1);
    conversation.resolve_elicitation("e", true);
    assert_eq!(tool(&conversation).state, ToolState::ApprovalResponded);
    assert!(conversation.elicitations().is_empty());
    conversation.apply(&output_item("c", "completed", "done"));
    assert_eq!(tool(&conversation).state, ToolState::OutputAvailable);

    // Denied.
    let mut conversation = fold(&[call_item("c", "in_progress"), approval_request("e")]);
    conversation.resolve_elicitation("e", false);
    conversation.apply(&output_item("c", "completed", "The user denied it."));
    assert_eq!(tool(&conversation).state, ToolState::OutputDenied);
    assert_eq!(
        tool(&conversation).error.as_deref(),
        Some("The user denied it.")
    );

    // A failed result, a failed call, a tool error.
    let conversation = fold(&[
        call_item("c", "completed"),
        output_item("c", "failed", "boom"),
    ]);
    assert_eq!(tool(&conversation).state, ToolState::OutputError);
    assert_eq!(tool(&conversation).error.as_deref(), Some("boom"));
    let conversation = fold(&[call_item("c", "failed")]);
    assert_eq!(tool(&conversation).state, ToolState::OutputError);
    let conversation = fold(&[
        call_item("c", "completed"),
        Event::Error {
            source: "tool".into(),
            tool_name: Some("uc__apply_grants".into()),
            code: "x".into(),
            message: "unreachable".into(),
        },
    ]);
    assert_eq!(tool(&conversation).state, ToolState::OutputError);

    // A client tool waits for its output.
    let conversation = fold(&[call_item("c", "action_required")]);
    assert_eq!(tool(&conversation).state, ToolState::InputAvailable);

    // A call seen twice is one call.
    let conversation = fold(&[call_item("c", "in_progress"), call_item("c", "completed")]);
    assert_eq!(conversation.messages()[0].parts.len(), 1);
}

#[test]
fn an_mcp_form_stays_an_open_elicitation() {
    // An MCP server's `elicitation/create` has no policy phase: no tool call is gated on it.
    let conversation = fold(&[
        call_item("c", "in_progress"),
        Event::ElicitationRequest(ElicitationRequest::from_json(
            json!({"elicitation_id": "e", "params": {"message": "Pick", "requestedSchema": {}}})
                .as_object()
                .unwrap(),
        )),
    ]);
    assert_eq!(tool(&conversation).state, ToolState::InputAvailable);
    assert_eq!(conversation.elicitations()[0].elicitation_id, "e");
}

#[test]
fn tool_names() {
    let cases = [
        ("uc__open_grant_form", Some("uc"), "open_grant_form"),
        (
            "mcp__omnigent__uc__open_grant_form",
            Some("uc"),
            "open_grant_form",
        ),
        ("uc-mcp__get_grants", Some("uc-mcp"), "get_grants"),
        ("sys_os_read", None, "sys_os_read"),
        ("__odd", None, "__odd"),
    ];
    for (name, server, tool) in cases {
        assert_eq!(ToolName::parse(name), ToolName { server, tool }, "{name}");
    }
}
