//! Messages and their roles.

use serde::{Deserialize, Serialize};

use crate::content::ContentBlock;
use crate::error::{Error, Result};
use crate::ids::{MessageUuid, PromptId, SessionId, ThreadId};

/// The author role of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    /// A transcript line whose kind Delta does not classify (e.g. summaries).
    Other,
}

impl Role {
    /// Parse a transcript `type` string into a role.
    ///
    /// Unknown kinds map to [`Role::Other`] rather than failing, because linear
    /// parent chains can include line kinds Delta does not model.
    pub fn from_transcript_type(value: &str) -> Self {
        match value {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "system" => Role::System,
            _ => Role::Other,
        }
    }

    /// The canonical lowercase label stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Other => "other",
        }
    }

    /// Parse a stored role label back into a [`Role`].
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "user" => Ok(Role::User),
            "assistant" => Ok(Role::Assistant),
            "system" => Ok(Role::System),
            "other" => Ok(Role::Other),
            other => Err(Error::InvalidVariant {
                kind: "Role",
                value: other.to_owned(),
            }),
        }
    }
}

/// A single message in the reconstructed thread graph.
///
/// A message has two distinct notions of parent:
///
/// - `linear_parent_uuid` is the transcript `parentUuid`: the model's real
///   context order, a single line. This may point at non user/assistant lines.
/// - `semantic_parent_uuid` is the `to:` reply edge. It is only set on user
///   branch messages and is usually `None`. A thread is a subtree of this
///   `to:` graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub uuid: MessageUuid,
    pub session_id: SessionId,
    /// The thread active at send time. Stored, never re-derived.
    pub thread_id: ThreadId,
    pub role: Role,
    pub linear_parent_uuid: Option<MessageUuid>,
    pub semantic_parent_uuid: Option<MessageUuid>,
    pub prompt_id: Option<PromptId>,
    /// Monotonic per-session ordering, mirroring transcript line order.
    pub seq: i64,
    /// Flattened plain-text view of the content, for quick display/search.
    pub content_text: Option<String>,
    /// The full ordered content blocks.
    pub content: Vec<ContentBlock>,
    /// ISO-8601 timestamp.
    pub created_at: String,
}

impl Message {
    /// Concatenate the text of all text/thinking blocks.
    pub fn flatten_text(blocks: &[ContentBlock]) -> Option<String> {
        let mut out = String::new();
        for block in blocks {
            if let Some(text) = block.as_text() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}
