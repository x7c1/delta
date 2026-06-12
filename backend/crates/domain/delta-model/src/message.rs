//! A single message in the reconstructed thread graph.

use crate::content::ContentBlock;
use crate::newtype::string_newtype;
use crate::role::Role;
use crate::session::SessionId;
use crate::thread::ThreadId;

string_newtype! {
    /// A transcript line uuid; the internal handle for a message.
    MessageUuid
}

string_newtype! {
    /// Claude Code's `promptId`, shared by all lines of one turn.
    PromptId
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// ISO-8601 timestamp, or `None` when the transcript line carried none.
    pub created_at: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_text_joins_text_and_thinking_blocks() {
        let blocks = vec![
            ContentBlock::Thinking {
                thinking: "hmm".into(),
            },
            ContentBlock::Text {
                text: "hello".into(),
            },
            ContentBlock::ToolUse {
                id: "t1".into(),
                name: "Bash".into(),
                input: serde_json::json!({"command": "ls"}),
            },
        ];
        assert_eq!(Message::flatten_text(&blocks).as_deref(), Some("hmm\nhello"));
        assert_eq!(Message::flatten_text(&[]), None);
    }
}
