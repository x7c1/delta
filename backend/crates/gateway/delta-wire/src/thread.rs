//! The wire form of [`Thread`].

use delta_model::Thread;
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of a thread record on the REST surface.
///
/// Mirrors the domain [`Thread`] field-for-field; see that type for the
/// semantics of each field. This wire twin carries the serialization concerns
/// the domain type must not know about: the field names on the wire and the
/// TypeScript export. Ids are plain `String`/`i64` here because that is
/// exactly what crosses the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "Thread")]
pub struct WireThread {
    pub id: i64,
    pub session_id: String,
    pub title: String,
    pub parent_thread_id: Option<i64>,
    /// The message this thread branches from, if any.
    pub root_message_uuid: Option<String>,
    /// ISO-8601 timestamp.
    pub created_at: String,
}

impl From<Thread> for WireThread {
    fn from(thread: Thread) -> Self {
        WireThread {
            id: thread.id.0,
            session_id: thread.session_id.0,
            title: thread.title,
            parent_thread_id: thread.parent_thread_id.map(|id| id.0),
            root_message_uuid: thread.root_message_uuid.map(|uuid| uuid.0),
            created_at: thread.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::{MessageUuid, SessionId, ThreadId};

    #[test]
    fn thread_serializes_with_the_rest_field_names() {
        let thread = Thread {
            id: ThreadId(2),
            session_id: SessionId::from("sess-1"),
            title: "branch".into(),
            parent_thread_id: Some(ThreadId(1)),
            root_message_uuid: Some(MessageUuid::from("uuid-root")),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert_eq!(
            serde_json::to_value(WireThread::from(thread)).unwrap(),
            serde_json::json!({
                "id": 2,
                "session_id": "sess-1",
                "title": "branch",
                "parent_thread_id": 1,
                "root_message_uuid": "uuid-root",
                "created_at": "2026-01-01T00:00:00Z",
            }),
        );
    }
}
