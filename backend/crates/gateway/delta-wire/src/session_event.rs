//! The wire form of [`SessionEvent`].

use delta_usecase::SessionEvent;
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of a session event on the `/ws` stream.
///
/// Mirrors the domain [`SessionEvent`] variant-for-variant; see that type for
/// the semantics of each event. This wire twin carries the serialization
/// concerns the domain type must not know about: the `kind` tag, the
/// snake_case variant names, and the TypeScript export. Ids are plain
/// `String`/`i64` here because that is exactly what crosses the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(rename = "SessionEvent")]
pub enum WireSessionEvent {
    /// The session was registered (first `UserPromptSubmit`).
    SessionRegistered {
        session_id: String,
    },
    /// A known, previously-closed session became live again (resumed).
    SessionOpened {
        session_id: String,
    },
    /// An open session was closed: its pane was torn down but its data remains.
    SessionClosed {
        session_id: String,
    },
    /// A queued send was confirmed as a turn start.
    TurnStarted {
        session_id: String,
        send_id: i64,
        matched_uuid: String,
    },
    /// External input was detected (typed directly into the pane).
    ExternalInput {
        session_id: String,
        prompt: String,
    },
    /// A response completed.
    TurnCompleted {
        session_id: String,
        stop_reason: Option<String>,
    },
    /// The in-flight turn was interrupted by the user (Escape / Ctrl-C).
    TurnInterrupted {
        session_id: String,
    },
    /// The transcript grew between hooks (continuous tail).
    TranscriptUpdated {
        session_id: String,
        thread_ids: Vec<i64>,
    },
    /// A tool permission prompt is imminent.
    PermissionRequested {
        session_id: String,
        request_id: i64,
        tool_name: String,
    },
    /// A previously-requested tool permission was resolved.
    PermissionResolved {
        session_id: String,
        request_id: i64,
    },
    /// A freshly-spawned session failed to come up before it ever registered.
    SpawnFailed {
        session_id: String,
        pane_token: String,
    },
}

impl From<SessionEvent> for WireSessionEvent {
    fn from(event: SessionEvent) -> Self {
        match event {
            SessionEvent::SessionRegistered { session_id } => Self::SessionRegistered {
                session_id: session_id.0,
            },
            SessionEvent::SessionOpened { session_id } => Self::SessionOpened {
                session_id: session_id.0,
            },
            SessionEvent::SessionClosed { session_id } => Self::SessionClosed {
                session_id: session_id.0,
            },
            SessionEvent::TurnStarted {
                session_id,
                send_id,
                matched_uuid,
            } => Self::TurnStarted {
                session_id: session_id.0,
                send_id,
                matched_uuid: matched_uuid.0,
            },
            SessionEvent::ExternalInput { session_id, prompt } => Self::ExternalInput {
                session_id: session_id.0,
                prompt,
            },
            SessionEvent::TurnCompleted {
                session_id,
                stop_reason,
            } => Self::TurnCompleted {
                session_id: session_id.0,
                stop_reason,
            },
            SessionEvent::TurnInterrupted { session_id } => Self::TurnInterrupted {
                session_id: session_id.0,
            },
            SessionEvent::TranscriptUpdated {
                session_id,
                thread_ids,
            } => Self::TranscriptUpdated {
                session_id: session_id.0,
                thread_ids: thread_ids.into_iter().map(|id| id.0).collect(),
            },
            SessionEvent::PermissionRequested {
                session_id,
                request_id,
                tool_name,
            } => Self::PermissionRequested {
                session_id: session_id.0,
                request_id,
                tool_name,
            },
            SessionEvent::PermissionResolved {
                session_id,
                request_id,
            } => Self::PermissionResolved {
                session_id: session_id.0,
                request_id,
            },
            SessionEvent::SpawnFailed {
                session_id,
                pane_token,
            } => Self::SpawnFailed {
                session_id: session_id.0,
                pane_token,
            },
        }
    }
}

/// Every `kind` discriminant, in declaration order, as serde puts it on the
/// wire.
///
/// Derived by serializing one sample of each variant, so the strings come from
/// the same serde attributes that produce the actual frames — there is no
/// second, hand-maintained list to drift.
pub fn event_kinds() -> Vec<String> {
    sample_events()
        .iter()
        .map(|event| {
            serde_json::to_value(event)
                .expect("wire event serializes")
                .get("kind")
                .and_then(|kind| kind.as_str())
                .expect("wire event carries a string `kind` tag")
                .to_owned()
        })
        .collect()
}

/// One sample of every variant, in declaration order.
fn sample_events() -> Vec<WireSessionEvent> {
    // Exhaustiveness guard: adding a `WireSessionEvent` variant fails this
    // match until the new variant also gets a sample below.
    fn covered(event: &WireSessionEvent) {
        match event {
            WireSessionEvent::SessionRegistered { .. }
            | WireSessionEvent::SessionOpened { .. }
            | WireSessionEvent::SessionClosed { .. }
            | WireSessionEvent::TurnStarted { .. }
            | WireSessionEvent::ExternalInput { .. }
            | WireSessionEvent::TurnCompleted { .. }
            | WireSessionEvent::TurnInterrupted { .. }
            | WireSessionEvent::TranscriptUpdated { .. }
            | WireSessionEvent::PermissionRequested { .. }
            | WireSessionEvent::PermissionResolved { .. }
            | WireSessionEvent::SpawnFailed { .. } => {}
        }
    }

    let session_id = || "sess-sample".to_owned();
    let samples = vec![
        WireSessionEvent::SessionRegistered {
            session_id: session_id(),
        },
        WireSessionEvent::SessionOpened {
            session_id: session_id(),
        },
        WireSessionEvent::SessionClosed {
            session_id: session_id(),
        },
        WireSessionEvent::TurnStarted {
            session_id: session_id(),
            send_id: 1,
            matched_uuid: "uuid-sample".to_owned(),
        },
        WireSessionEvent::ExternalInput {
            session_id: session_id(),
            prompt: "prompt".to_owned(),
        },
        WireSessionEvent::TurnCompleted {
            session_id: session_id(),
            stop_reason: None,
        },
        WireSessionEvent::TurnInterrupted {
            session_id: session_id(),
        },
        WireSessionEvent::TranscriptUpdated {
            session_id: session_id(),
            thread_ids: vec![1],
        },
        WireSessionEvent::PermissionRequested {
            session_id: session_id(),
            request_id: 1,
            tool_name: "Bash".to_owned(),
        },
        WireSessionEvent::PermissionResolved {
            session_id: session_id(),
            request_id: 1,
        },
        WireSessionEvent::SpawnFailed {
            session_id: session_id(),
            pane_token: "delta-sample".to_owned(),
        },
    ];
    for event in &samples {
        covered(event);
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::{MessageUuid, SessionId, ThreadId};

    fn json(event: &WireSessionEvent) -> serde_json::Value {
        serde_json::to_value(event).unwrap()
    }

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
            }),
            serde_json::json!({ "kind": "turn_interrupted", "session_id": "sess-1" }),
        );
    }

    #[test]
    fn spawn_failed_serializes_with_id_and_pane_token() {
        assert_eq!(
            json(&WireSessionEvent::SpawnFailed {
                session_id: "sess-1".into(),
                pane_token: "delta-1".into(),
            }),
            serde_json::json!({
                "kind": "spawn_failed",
                "session_id": "sess-1",
                "pane_token": "delta-1",
            }),
        );
    }

    #[test]
    fn permission_requested_and_resolved_serialize_as_tagged_events() {
        assert_eq!(
            json(&WireSessionEvent::PermissionRequested {
                session_id: "sess-1".into(),
                request_id: 7,
                tool_name: "Bash".into(),
            }),
            serde_json::json!({
                "kind": "permission_requested",
                "session_id": "sess-1",
                "request_id": 7,
                "tool_name": "Bash",
            }),
        );
        assert_eq!(
            json(&WireSessionEvent::PermissionResolved {
                session_id: "sess-1".into(),
                request_id: 7,
            }),
            serde_json::json!({
                "kind": "permission_resolved",
                "session_id": "sess-1",
                "request_id": 7,
            }),
        );
    }

    #[test]
    fn turn_events_keep_their_payload_fields_on_the_wire() {
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::TurnStarted {
                session_id: SessionId::from("sess-1"),
                send_id: 42,
                matched_uuid: MessageUuid::from("uuid-1"),
            })),
            serde_json::json!({
                "kind": "turn_started",
                "session_id": "sess-1",
                "send_id": 42,
                "matched_uuid": "uuid-1",
            }),
        );
        assert_eq!(
            json(&WireSessionEvent::from(SessionEvent::TurnCompleted {
                session_id: SessionId::from("sess-1"),
                stop_reason: None,
            })),
            serde_json::json!({
                "kind": "turn_completed",
                "session_id": "sess-1",
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
    fn event_kinds_lists_every_variant_in_declaration_order() {
        assert_eq!(
            event_kinds(),
            [
                "session_registered",
                "session_opened",
                "session_closed",
                "turn_started",
                "external_input",
                "turn_completed",
                "turn_interrupted",
                "transcript_updated",
                "permission_requested",
                "permission_resolved",
                "spawn_failed",
            ],
        );
    }
}
