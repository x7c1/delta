//! A parsed transcript line, before Delta assigns it a thread.

use delta_model::{ContentBlock, Message, MessageUuid, PromptId, Role};

/// A parsed transcript line, before Delta assigns it a thread.
///
/// The transcript gateway produces these from the raw JSONL; the Interactor
/// turns them into [`Message`] values by attaching the active `thread_id` and
/// any known semantic parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptMessage {
    pub uuid: MessageUuid,
    pub role: Role,
    /// The transcript `parentUuid` (linear/model context order).
    pub linear_parent_uuid: Option<MessageUuid>,
    pub prompt_id: Option<PromptId>,
    pub content: Vec<ContentBlock>,
    /// ISO-8601 timestamp from the transcript line, if present.
    pub created_at: Option<String>,
    /// The message's absolute 0-based line index in the transcript file.
    ///
    /// Used as the persisted `seq`, so it reflects the line's true file
    /// position even when earlier lines were skipped (blank, no-uuid, or
    /// unparsable). The reader assigns this; the Interactor persists it.
    pub seq: i64,
}

impl TranscriptMessage {
    /// The flattened text view of this line's content, if any.
    pub fn flatten_text(&self) -> Option<String> {
        Message::flatten_text(&self.content)
    }
}
