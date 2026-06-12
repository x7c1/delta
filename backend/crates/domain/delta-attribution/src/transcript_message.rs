//! A parsed transcript line, before Delta assigns it a thread.

use delta_model::{ContentBlock, Message, MessageUuid, PromptId, Role};

/// A parsed transcript line, before Delta assigns it a thread.
///
/// The transcript gateway produces these from the raw JSONL; the attribution
/// fold turns them into [`Message`] values by attaching the active
/// `thread_id` and any known semantic parent.
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
    /// unparsable). The reader assigns this; the fold persists it.
    pub seq: i64,
    /// True when this line is a Claude Code `queued_command` attachment: a
    /// prompt the user composed while a turn was in flight, which Claude records
    /// *only* as this attachment (never as a normal `type: "user"` line) and
    /// injects programmatically once the turn yields.
    ///
    /// It is surfaced as a [`Role::User`] message so it both displays and feeds
    /// the send-correlation path. The flag matters for the *uncorrelated* case:
    /// a queued command that matches no queued send must NOT reset attribution
    /// to `main` the way stray pane typing does, because it is a programmatic
    /// injection (e.g. a background task notification), not external input — it
    /// inherits the active thread instead.
    pub is_queued_command: bool,
}

impl TranscriptMessage {
    /// The flattened text view of this line's content, if any.
    pub fn flatten_text(&self) -> Option<String> {
        Message::flatten_text(&self.content)
    }
}
