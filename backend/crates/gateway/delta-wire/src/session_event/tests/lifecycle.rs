//! Session registration, opening, closing, removal and turn interruption.

use super::*;

#[test]
fn open_and_closed_serialize_as_id_routed_tagged_events() {
    assert_eq!(
        json(&WireSessionEvent::SessionOpened {
            session_id: "sess-1".into(),
        }),
        serde_json::json!({ "kind": "session_opened", "session_id": "sess-1" }),
    );
    assert_eq!(
        json(&WireSessionEvent::SessionClosed {
            session_id: "sess-1".into(),
        }),
        serde_json::json!({ "kind": "session_closed", "session_id": "sess-1" }),
    );
}

/// A removal converts from the domain event and lands on the wire as its
/// own `kind`, never as `session_closed`: a client hearing this one has to
/// drop the row, which is not what closing means.
#[test]
fn removed_converts_from_the_domain_event_and_keeps_its_own_kind() {
    let wire = WireSessionEvent::from(SessionEvent::SessionRemoved {
        session_id: SessionId::from("sess-1"),
    });
    assert_eq!(
        wire,
        WireSessionEvent::SessionRemoved {
            session_id: "sess-1".to_owned(),
        },
    );
    assert_eq!(
        json(&wire),
        serde_json::json!({ "kind": "session_removed", "session_id": "sess-1" }),
    );
}

#[test]
fn registered_keeps_its_wire_shape() {
    assert_eq!(
        json(&WireSessionEvent::SessionRegistered {
            session_id: "sess-1".into(),
        }),
        serde_json::json!({ "kind": "session_registered", "session_id": "sess-1" }),
    );
}

#[test]
fn turn_interrupted_serializes_as_id_routed_tagged_event() {
    assert_eq!(
        json(&WireSessionEvent::TurnInterrupted {
            session_id: "sess-1".into(),
            thread_id: Some(4),
        }),
        serde_json::json!({
            "kind": "turn_interrupted",
            "session_id": "sess-1",
            "thread_id": 4,
        }),
    );
}
