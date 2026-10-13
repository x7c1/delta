//! Sends that were dispatched or parked.

use super::*;

#[test]
fn send_dispatched_serializes_with_id_and_send_id() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::SendDispatched {
            session_id: SessionId::from("sess-1"),
            send_id: 42,
        })),
        serde_json::json!({
            "kind": "send_dispatched",
            "session_id": "sess-1",
            "send_id": 42,
        }),
    );
}

#[test]
fn send_parked_serializes_with_the_undelivered_text() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::SendParked {
            session_id: SessionId::from("sess-1"),
            send_id: 42,
            text: "read this\n/home/dev/pictures/shot.png".to_owned(),
        })),
        serde_json::json!({
            "kind": "send_parked",
            "session_id": "sess-1",
            "send_id": 42,
            "text": "read this\n/home/dev/pictures/shot.png",
        }),
    );
}
