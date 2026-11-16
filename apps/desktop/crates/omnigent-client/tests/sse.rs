//! The SSE decoder and event parse, over every way a body can be cut into chunks.

use futures::StreamExt as _;
use omnigent_client::sse::{Frame, SseDecoder};
use omnigent_client::{Event, EventStream, OutputItem, ResponseStatus, Role};

const GRANT_TURN: &str = include_str!("../fixtures/grant-turn.sse");

fn decode(chunks: &[&[u8]]) -> Vec<Frame> {
    let mut decoder = SseDecoder::new();
    let mut frames = Vec::new();
    for chunk in chunks {
        frames.extend(decoder.push(chunk));
    }
    frames.extend(decoder.finish());
    frames
}

fn event(event: Option<&str>, data: &str) -> Frame {
    Frame::Event {
        event: event.map(str::to_owned),
        data: data.to_owned(),
    }
}

#[test]
fn frames_whatever_the_chunking() {
    // CRLF, LF, and a multi-byte character, cut at every byte.
    let bodies = [GRANT_TURN.to_owned(), GRANT_TURN.replace('\n', "\r\n")];
    for body in &bodies {
        let bytes = body.as_bytes();
        let whole = decode(&[bytes]);
        // Omnigent's stream doesn't end: no `[DONE]`, the recording just stops.
        assert_eq!(whole.len(), 28, "frames of the fixture");
        assert!(!whole.contains(&Frame::Done));
        for cut in 0..=bytes.len() {
            let (a, b) = bytes.split_at(cut);
            assert_eq!(decode(&[a, b]), whole, "cut at byte {cut}");
        }
        let bytewise: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(decode(&bytewise), whole, "one byte per chunk");
        let threes: Vec<&[u8]> = bytes.chunks(3).collect();
        assert_eq!(decode(&threes), whole, "three bytes per chunk");
    }
}

#[test]
fn frame_cases() {
    let cases: &[(&str, &str, Vec<Frame>)] = &[
        (
            "multi-line data joins with \\n",
            "event: x\ndata: {\"a\":\ndata: 1}\n\n",
            vec![event(Some("x"), "{\"a\":\n1}")],
        ),
        (
            "no space after the colon",
            "event:x\ndata:{}\n\n",
            vec![event(Some("x"), "{}")],
        ),
        (
            "comments and id/retry are ignored",
            ": ping\nid: 7\nretry: 100\nevent: x\ndata: {}\n\n",
            vec![event(Some("x"), "{}")],
        ),
        (
            "an event without a name",
            "data: {}\n\n",
            vec![event(None, "{}")],
        ),
        (
            "a blank line without data dispatches nothing",
            "event: x\n\n\n",
            vec![],
        ),
        (
            "nothing after [DONE]",
            "data: [DONE]\nevent: x\ndata: {}\n\n",
            vec![Frame::Done],
        ),
        (
            "[DONE] without its blank line",
            "event: x\ndata: {}\n\ndata: [DONE]",
            vec![event(Some("x"), "{}"), Frame::Done],
        ),
        (
            "a last frame without its blank line",
            "event: x\ndata: {}",
            vec![event(Some("x"), "{}")],
        ),
        (
            "a bare CR before the LF of the next chunk",
            "event: x\r\ndata: {}\r\n\r\n",
            vec![event(Some("x"), "{}")],
        ),
    ];
    for (name, body, expected) in cases {
        assert_eq!(&decode(&[body.as_bytes()]), expected, "{name}");
        let bytewise: Vec<&[u8]> = body.as_bytes().chunks(1).collect();
        assert_eq!(&decode(&bytewise), expected, "{name}, one byte per chunk");
    }
}

#[test]
fn event_parse() {
    assert_eq!(
        Event::parse(
            Some("response.TaskStatus.COMPLETED"),
            r#"{"type":"response.completed","response":{"id":"resp_1"}}"#
        ),
        Event::Response {
            status: ResponseStatus::Completed,
            response_id: Some("resp_1".into()),
            source: None,
            error: None,
        },
    );
    assert_eq!(
        Event::parse(
            None,
            r#"{"type":"response.failed","source":"llm","response":{"id":"r","error":{"code":"x","message":"boom"}}}"#
        ),
        Event::Response {
            status: ResponseStatus::Failed,
            response_id: Some("r".into()),
            source: Some("llm".into()),
            error: Some("boom".into()),
        },
    );
    assert_eq!(
        Event::parse(Some("response.output_text.delta"), r#"{"delta":"hi"}"#),
        Event::OutputTextDelta { delta: "hi".into() },
    );
    assert_eq!(
        Event::parse(
            Some("response.error"),
            r#"{"type":"response.error","source":"tool","tool_name":"uc__get_grants","error":{"code":"timeout","message":"slow"}}"#
        ),
        Event::Error {
            source: "tool".into(),
            tool_name: Some("uc__get_grants".into()),
            code: "timeout".into(),
            message: "slow".into(),
        },
    );
    let Event::ElicitationRequest(elicitation) = Event::parse(
        Some("response.elicitation_request"),
        r#"{"type":"response.elicitation_request","elicitation_id":"elicit_1","method":"elicitation/create",
            "params":{"mode":"form","message":"Approve?","requestedSchema":{"type":"object"},"phase":"pre_tool_use","policy_name":"ask"}}"#,
    ) else {
        panic!("an elicitation");
    };
    assert_eq!(elicitation.elicitation_id, "elicit_1");
    assert_eq!(elicitation.message, "Approve?");
    assert_eq!(elicitation.phase.as_deref(), Some("pre_tool_use"));
    assert_eq!(
        elicitation.requested_schema,
        Some(serde_json::json!({"type": "object"}))
    );
    // An unknown type, or a payload that isn't JSON, passes through.
    assert_eq!(
        Event::parse(Some("session.usage"), r#"{"data":{"total_tokens":1}}"#),
        Event::Unknown {
            kind: "session.usage".into(),
            data: serde_json::json!({"data": {"total_tokens": 1}}),
        },
    );
    assert_eq!(
        Event::parse(Some("x"), "not json"),
        Event::Unknown {
            kind: "x".into(),
            data: serde_json::Value::String("not json".into()),
        },
    );
}

#[tokio::test]
async fn event_stream_over_chunks() {
    let chunks: Vec<reqwest::Result<bytes::Bytes>> = GRANT_TURN
        .as_bytes()
        .chunks(7)
        .map(|c| Ok(bytes::Bytes::copy_from_slice(c)))
        .collect();
    let events: Vec<Event> = EventStream::from_body(futures::stream::iter(chunks))
        .map(|e| e.expect("event"))
        .collect()
        .await;
    assert_eq!(events.len(), 28, "every frame");
    assert!(events.iter().any(|e| matches!(
        e,
        Event::InputConsumed {
            item: OutputItem::Message {
                role: Role::User,
                ..
            },
            ..
        }
    )));
    let call = events
        .iter()
        .find_map(|e| match e {
            Event::OutputItemDone(OutputItem::FunctionCall { name, call_id, .. }) => {
                Some((name.as_str(), call_id.as_str()))
            }
            _ => None,
        })
        .expect("the tool call");
    // The form as the agent's YAML names it: `<server>__<tool>`, no `mcp__omnigent__`.
    assert_eq!(
        call,
        ("uc__open_grant_form", "toolu_bdrk_018cHzjXgmVZAj2ENDEwrPuc")
    );
    let completed = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::Response {
                    status: ResponseStatus::Completed,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        completed, 2,
        "two turns: the request and the form's outcome"
    );
}
