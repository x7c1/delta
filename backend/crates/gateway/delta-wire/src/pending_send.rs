//! The wire form of [`PendingSend`].

use delta_model::{PendingSend, PendingSendStatus};
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of a queued send's correlation status.
///
/// Mirrors the domain [`PendingSendStatus`] variant-for-variant; see that type
/// for the semantics of each status. This wire twin carries the serialization
/// concerns the domain type must not know about: the lowercase variant names
/// and the TypeScript export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "PendingSendStatus")]
pub enum WirePendingSendStatus {
    Deferred,
    Pending,
    Matched,
    Cancelled,
}

impl From<PendingSendStatus> for WirePendingSendStatus {
    fn from(status: PendingSendStatus) -> Self {
        match status {
            PendingSendStatus::Deferred => WirePendingSendStatus::Deferred,
            PendingSendStatus::Pending => WirePendingSendStatus::Pending,
            PendingSendStatus::Matched => WirePendingSendStatus::Matched,
            PendingSendStatus::Cancelled => WirePendingSendStatus::Cancelled,
        }
    }
}

/// JSON shape of a queued send on the REST surface.
///
/// Mirrors the domain [`PendingSend`] field-for-field; see that type for the
/// semantics of each field. This wire twin carries the serialization concerns
/// the domain type must not know about: the field names on the wire and the
/// TypeScript export. Ids are plain `String`/`i64` here because that is
/// exactly what crosses the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PendingSend")]
pub struct WirePendingSend {
    pub id: i64,
    pub session_id: String,
    pub thread_id: i64,
    /// When branching, the message this reply is `to:`.
    pub semantic_parent_uuid: Option<String>,
    pub text: String,
    /// Optional short quote injected as `additionalContext` to locate the reply.
    pub locator_quote: Option<String>,
    pub status: WirePendingSendStatus,
    /// The transcript message uuid once matched.
    pub matched_uuid: Option<String>,
    /// ISO-8601 timestamp.
    pub created_at: String,
}

impl From<PendingSend> for WirePendingSend {
    fn from(send: PendingSend) -> Self {
        WirePendingSend {
            id: send.id,
            session_id: send.session_id.0,
            thread_id: send.thread_id.0,
            semantic_parent_uuid: send.semantic_parent_uuid.map(|uuid| uuid.0),
            text: send.text,
            locator_quote: send.locator_quote,
            status: send.status.into(),
            matched_uuid: send.matched_uuid.map(|uuid| uuid.0),
            created_at: send.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::{SessionId, ThreadId};

    #[test]
    fn pending_send_serializes_with_the_rest_field_names() {
        let send = PendingSend {
            id: 42,
            session_id: SessionId::from("sess-1"),
            thread_id: ThreadId(1),
            semantic_parent_uuid: None,
            text: "hi".into(),
            locator_quote: Some("quote".into()),
            status: PendingSendStatus::Pending,
            matched_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert_eq!(
            serde_json::to_value(WirePendingSend::from(send)).unwrap(),
            serde_json::json!({
                "id": 42,
                "session_id": "sess-1",
                "thread_id": 1,
                "semantic_parent_uuid": null,
                "text": "hi",
                "locator_quote": "quote",
                "status": "pending",
                "matched_uuid": null,
                "created_at": "2026-01-01T00:00:00Z",
            }),
        );
    }

    #[test]
    fn every_status_serializes_lowercase() {
        for (status, expected) in [
            (PendingSendStatus::Deferred, "deferred"),
            (PendingSendStatus::Pending, "pending"),
            (PendingSendStatus::Matched, "matched"),
            (PendingSendStatus::Cancelled, "cancelled"),
        ] {
            assert_eq!(
                serde_json::to_value(WirePendingSendStatus::from(status)).unwrap(),
                serde_json::json!(expected),
            );
        }
    }
}
