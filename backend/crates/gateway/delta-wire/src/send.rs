//! The wire form of [`Send`].

use delta_model::Send;
use serde::Serialize;
use ts_rs::TS;

use crate::send_status::WireSendStatus;

/// JSON shape of a queued send on the REST surface.
///
/// Mirrors the domain [`Send`] field-for-field; see that type for the
/// semantics of each field. This wire twin carries the serialization concerns
/// the domain type must not know about: the field names on the wire and the
/// TypeScript export. Ids are plain `String`/`i64` here because that is
/// exactly what crosses the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "Send")]
pub struct WireSend {
    pub id: i64,
    pub session_id: String,
    pub thread_id: i64,
    /// When branching, the message this reply is `to:`.
    pub semantic_parent_uuid: Option<String>,
    pub text: String,
    /// Optional short quote injected as `additionalContext` to locate the reply.
    pub locator_quote: Option<String>,
    pub status: WireSendStatus,
    /// The transcript message uuid once matched.
    pub matched_uuid: Option<String>,
    /// ISO-8601 timestamp.
    pub created_at: String,
    /// ISO-8601 timestamp marking this `queued` row as held in the queue until
    /// the user releases it — set either by the boot-time restore (a row a
    /// dead server process left `dispatched`) or by the echo-deadline park (a
    /// row whose keystrokes were swallowed without a trace twice running). A
    /// held send never auto-dispatches: the browser renders it with explicit
    /// Send (`POST /api/sends/{id}/release`) and Cancel actions. `null` on the
    /// normal send path.
    pub held_at: Option<String>,
}

impl From<Send> for WireSend {
    fn from(send: Send) -> Self {
        WireSend {
            id: send.id,
            session_id: send.session_id.0,
            thread_id: send.thread_id.0,
            semantic_parent_uuid: send.semantic_parent_uuid.map(|uuid| uuid.0),
            text: send.text,
            locator_quote: send.locator_quote,
            status: send.status.into(),
            matched_uuid: send.matched_uuid.map(|uuid| uuid.0),
            created_at: send.created_at,
            held_at: send.held_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::SendStatus;

    use delta_model::{SessionId, ThreadId};

    #[test]
    fn send_serializes_with_the_rest_field_names() {
        let send = Send {
            id: 42,
            session_id: SessionId::from("sess-1"),
            thread_id: ThreadId(1),
            semantic_parent_uuid: None,
            text: "hi".into(),
            locator_quote: Some("quote".into()),
            status: SendStatus::Dispatched,
            matched_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            held_at: None,
        };
        assert_eq!(
            serde_json::to_value(WireSend::from(send)).unwrap(),
            serde_json::json!({
                "id": 42,
                "session_id": "sess-1",
                "thread_id": 1,
                "semantic_parent_uuid": null,
                "text": "hi",
                "locator_quote": "quote",
                "status": "dispatched",
                "matched_uuid": null,
                "created_at": "2026-01-01T00:00:00Z",
                "held_at": null,
            }),
        );
    }
}
