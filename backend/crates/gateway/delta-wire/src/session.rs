//! The wire form of [`Session`].

use delta_model::{Session, SessionStatus};
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of a session's lifecycle status.
///
/// Mirrors the domain [`SessionStatus`] variant-for-variant; this wire twin
/// carries the serialization concerns the domain type must not know about:
/// the lowercase variant names and the TypeScript export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "SessionStatus")]
pub enum WireSessionStatus {
    Active,
    Ended,
}

impl From<SessionStatus> for WireSessionStatus {
    fn from(status: SessionStatus) -> Self {
        match status {
            SessionStatus::Active => WireSessionStatus::Active,
            SessionStatus::Ended => WireSessionStatus::Ended,
        }
    }
}

/// JSON shape of a session record on the REST surface.
///
/// Mirrors the domain [`Session`] field-for-field; see that type for the
/// semantics of each field. This wire twin carries the serialization concerns
/// the domain type must not know about: the field names on the wire and the
/// TypeScript export. Ids are plain `String` here because that is exactly what
/// crosses the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "Session")]
pub struct WireSession {
    pub id: String,
    pub cwd: String,
    pub transcript_path: String,
    pub title: Option<String>,
    pub status: WireSessionStatus,
    /// ISO-8601 timestamp.
    pub created_at: String,
}

impl From<Session> for WireSession {
    fn from(session: Session) -> Self {
        WireSession {
            id: session.id.0,
            cwd: session.cwd,
            transcript_path: session.transcript_path,
            title: session.title,
            status: session.status.into(),
            created_at: session.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::SessionId;

    #[test]
    fn session_serializes_with_the_rest_field_names() {
        let session = Session {
            id: SessionId::from("sess-1"),
            cwd: "/work/delta".into(),
            transcript_path: "/tmp/t.jsonl".into(),
            title: None,
            status: SessionStatus::Active,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert_eq!(
            serde_json::to_value(WireSession::from(session)).unwrap(),
            serde_json::json!({
                "id": "sess-1",
                "cwd": "/work/delta",
                "transcript_path": "/tmp/t.jsonl",
                "title": null,
                "status": "active",
                "created_at": "2026-01-01T00:00:00Z",
            }),
        );
    }

    #[test]
    fn status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_value(WireSessionStatus::Ended).unwrap(),
            serde_json::json!("ended"),
        );
    }
}
