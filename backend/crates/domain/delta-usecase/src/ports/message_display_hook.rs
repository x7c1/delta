//! Payload of a `MessageDisplay` hook.

use delta_model::SessionId;

/// Payload of a `MessageDisplay` hook: one chunk of the in-flight assistant
/// message's visible text, delivered live while the turn is still generating.
///
/// See [`crate::SessionEvent::AssistantStreaming`] for how the chunks are
/// accumulated and surfaced as a provisional live preview that the persisted
/// transcript message later supersedes.
#[derive(Debug, Clone)]
pub struct MessageDisplayHook {
    pub session_id: SessionId,
    /// The display message these chunks belong to (stable across one message's
    /// fires; not a transcript id).
    pub message_id: String,
    /// The chunk's position within the message, increasing 0, 1, 2, …
    pub index: u32,
    /// True only on the last chunk of a message.
    pub final_: bool,
    /// The visible assistant text chunk.
    pub delta: String,
}
