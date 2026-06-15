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
    /// prompt the user composed while a turn was in flight, which **older**
    /// claude versions recorded *only* as this attachment (never as a normal
    /// `type: "user"` line) and injected programmatically once the turn
    /// yielded.
    ///
    /// LEGACY FORMAT COMPATIBILITY — keep this flag and its handling. Current
    /// claude records a uuid-less `queue-operation` line instead (skipped by
    /// the parser) and replays the prompt as a plain user line that flows the
    /// normal attribution path (see the queued-prompt drift note in
    /// docs/guides/development.md); but transcripts recorded by older
    /// versions are still resumed and viewed.
    ///
    /// It is surfaced as a [`Role::User`] message so it both displays and feeds
    /// the send-correlation path. The flag matters for the *uncorrelated* case:
    /// a queued command that matches no queued send must NOT reset attribution
    /// to `main` the way stray pane typing does, because it is a programmatic
    /// injection (e.g. a background task notification), not external input — it
    /// inherits the active thread instead.
    pub is_queued_command: bool,
    /// True when Claude Code flagged this line `isApiErrorMessage`: a synthetic
    /// assistant line it writes when a turn ends on an API error (a
    /// usage/session limit, a rate limit, or any other API failure) rather than
    /// completing normally.
    ///
    /// Such a turn-end fires **no** `Stop` hook and writes **no** interrupt
    /// marker, so without this flag the per-session turn state machine would
    /// stay in flight forever and every later send would defer to `queued` and
    /// never dispatch. The attribution fold reads this flag to emit a
    /// transcript-driven turn-end effect ([`Effect::TurnAborted`]), parallel to
    /// how the interrupt marker yields [`Effect::TurnInterrupted`]. It is keyed
    /// on the structural flag, never on the human-readable error text, so it
    /// covers every synthetic API-error turn-end generically and is
    /// locale-independent.
    ///
    /// [`Effect::TurnAborted`]: crate::Effect::TurnAborted
    /// [`Effect::TurnInterrupted`]: crate::Effect::TurnInterrupted
    pub is_api_error: bool,
}

impl TranscriptMessage {
    /// The flattened text view of this line's content, if any.
    pub fn flatten_text(&self) -> Option<String> {
        Message::flatten_text(&self.content)
    }
}
