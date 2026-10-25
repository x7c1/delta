//! Turn events and assistant streaming.

use super::*;

#[test]
fn turn_events_keep_their_payload_fields_on_the_wire() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::TurnStarted {
            session_id: SessionId::from("sess-1"),
            send_id: Some(42),
            thread_id: ThreadId(2),
            matched_uuid: Some(MessageUuid::from("uuid-1")),
        })),
        serde_json::json!({
            "kind": "turn_started",
            "session_id": "sess-1",
            "send_id": 42,
            "thread_id": 2,
            "matched_uuid": "uuid-1",
        }),
    );
    // A turn no send was consumed for (pane typing, a background-task
    // notification) names neither a send nor a matched line.
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::TurnStarted {
            session_id: SessionId::from("sess-1"),
            send_id: None,
            thread_id: ThreadId(1),
            matched_uuid: None,
        })),
        serde_json::json!({
            "kind": "turn_started",
            "session_id": "sess-1",
            "send_id": null,
            "thread_id": 1,
            "matched_uuid": null,
        }),
    );
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::TurnCompleted {
            session_id: SessionId::from("sess-1"),
            thread_id: Some(ThreadId(2)),
            stop_reason: None,
        })),
        serde_json::json!({
            "kind": "turn_completed",
            "session_id": "sess-1",
            "thread_id": 2,
            "stop_reason": null,
        }),
    );
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::TranscriptUpdated {
            session_id: SessionId::from("sess-1"),
            thread_ids: vec![ThreadId(3), ThreadId(5)],
        })),
        serde_json::json!({
            "kind": "transcript_updated",
            "session_id": "sess-1",
            "thread_ids": [3, 5],
        }),
    );
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::ExternalInput {
            session_id: SessionId::from("sess-1"),
            prompt: "typed in pane".into(),
        })),
        serde_json::json!({
            "kind": "external_input",
            "session_id": "sess-1",
            "prompt": "typed in pane",
        }),
    );
}

#[test]
fn assistant_streaming_serializes_with_its_chunk_payload() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::AssistantStreaming {
            session_id: SessionId::from("sess-1"),
            thread_id: ThreadId(3),
            message_id: "msg-7".into(),
            index: 2,
            final_: true,
            delta: "hello".into(),
        })),
        serde_json::json!({
            "kind": "assistant_streaming",
            "session_id": "sess-1",
            "thread_id": 3,
            "message_id": "msg-7",
            "index": 2,
            "final": true,
            "delta": "hello",
        }),
    );
}
