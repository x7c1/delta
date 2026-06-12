//! The wire form of [`Message`].

use delta_model::{Message, Role};
use serde::Serialize;
use ts_rs::TS;

use crate::content_block::WireContentBlock;

/// JSON shape of a message's author role.
///
/// Mirrors the domain [`Role`] variant-for-variant; see that type for the
/// semantics of each role. This wire twin carries the serialization concerns
/// the domain type must not know about: the lowercase variant names and the
/// TypeScript export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "MessageRole")]
pub enum WireRole {
    User,
    Assistant,
    System,
    Meta,
    Other,
}

impl From<Role> for WireRole {
    fn from(role: Role) -> Self {
        match role {
            Role::User => WireRole::User,
            Role::Assistant => WireRole::Assistant,
            Role::System => WireRole::System,
            Role::Meta => WireRole::Meta,
            Role::Other => WireRole::Other,
        }
    }
}

/// JSON shape of a message on the REST surface.
///
/// Mirrors the domain [`Message`] field-for-field; see that type for the
/// semantics of each field (in particular the two parent uuids). This wire
/// twin carries the serialization concerns the domain type must not know
/// about: the field names on the wire and the TypeScript export. Ids are plain
/// `String`/`i64` here because that is exactly what crosses the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "Message")]
pub struct WireMessage {
    pub uuid: String,
    pub session_id: String,
    pub thread_id: i64,
    pub role: WireRole,
    pub linear_parent_uuid: Option<String>,
    pub semantic_parent_uuid: Option<String>,
    pub prompt_id: Option<String>,
    /// Monotonic per-session ordering, mirroring transcript line order.
    pub seq: i64,
    /// Flattened plain-text view of the content, for quick display/search.
    pub content_text: Option<String>,
    /// The full ordered content blocks.
    pub content: Vec<WireContentBlock>,
    /// ISO-8601 timestamp; empty when the transcript line carried none. The
    /// domain stores `None` for a missing timestamp, but the wire keeps the
    /// pre-existing empty-string contract so the browser shape is unchanged.
    pub created_at: String,
}

impl From<Message> for WireMessage {
    fn from(message: Message) -> Self {
        WireMessage {
            uuid: message.uuid.0,
            session_id: message.session_id.0,
            thread_id: message.thread_id.0,
            role: message.role.into(),
            linear_parent_uuid: message.linear_parent_uuid.map(|uuid| uuid.0),
            semantic_parent_uuid: message.semantic_parent_uuid.map(|uuid| uuid.0),
            prompt_id: message.prompt_id.map(|id| id.0),
            seq: message.seq,
            content_text: message.content_text,
            content: message
                .content
                .into_iter()
                .map(WireContentBlock::from)
                .collect(),
            created_at: message.created_at.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::{ContentBlock, MessageUuid, SessionId, ThreadId};

    #[test]
    fn message_serializes_with_the_rest_field_names() {
        let message = Message {
            uuid: MessageUuid::from("uuid-1"),
            session_id: SessionId::from("sess-1"),
            thread_id: ThreadId(1),
            role: Role::Assistant,
            linear_parent_uuid: Some(MessageUuid::from("uuid-0")),
            semantic_parent_uuid: None,
            prompt_id: None,
            seq: 3,
            content_text: Some("hello".into()),
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
            created_at: Some("2026-01-01T00:00:00Z".into()),
        };
        assert_eq!(
            serde_json::to_value(WireMessage::from(message)).unwrap(),
            serde_json::json!({
                "uuid": "uuid-1",
                "session_id": "sess-1",
                "thread_id": 1,
                "role": "assistant",
                "linear_parent_uuid": "uuid-0",
                "semantic_parent_uuid": null,
                "prompt_id": null,
                "seq": 3,
                "content_text": "hello",
                "content": [{ "type": "text", "text": "hello" }],
                "created_at": "2026-01-01T00:00:00Z",
            }),
        );
    }

    #[test]
    fn role_serializes_lowercase() {
        assert_eq!(
            serde_json::to_value(WireRole::from(Role::Meta)).unwrap(),
            serde_json::json!("meta"),
        );
    }
}
