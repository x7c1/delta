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
///
/// `response_time_ms` is an `f64`, so this type derives only `PartialEq` — a
/// float cannot implement `Eq`/`Hash`.
#[derive(Debug, Clone, PartialEq)]
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
    /// The model that produced this message (the transcript's `message.model`),
    /// or `None` for non-assistant lines and shapes that carry no model. This is
    /// the historical, per-message model — distinct from the user's *current*
    /// model selection reported by the status line.
    pub model: Option<String>,
    /// The git branch active at this turn (the transcript's top-level
    /// `gitBranch`), or `None` when absent. Unlike `cwd`, this can change
    /// mid-session (e.g. a `git checkout` between turns).
    pub git_branch: Option<String>,
    /// The working directory at this turn (the transcript's top-level `cwd`), or
    /// `None` when absent. Effectively fixed for a session's lifetime.
    pub cwd: Option<String>,
    /// The turn's response time in milliseconds, correlated from the turn's
    /// `system`/`turn_duration` line, or `None` when no duration was recorded
    /// for the turn. An `f64`, so this type cannot derive `Eq`/`Hash`.
    pub response_time_ms: Option<f64>,
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
        assert_eq!(
            Message::flatten_text(&blocks).as_deref(),
            Some("hmm\nhello")
        );
        assert_eq!(Message::flatten_text(&[]), None);
    }
}
