//! `MessageDisplay` payload.

use serde::{Deserialize, Serialize};

/// `MessageDisplay` payload.
///
/// Claude Code fires this hook repeatedly while an assistant message is being
/// generated — before the transcript JSONL is flushed and before any blocking
/// tool prompt blocks. Each fire carries one chunk of the visible assistant
/// text (`delta`) at a monotonically increasing `index`, all sharing one
/// `message_id`; only the last fire of a message has `final: true`. The chunks
/// are per display-segment (a line / paragraph), not per token.
///
/// The ids here (`message_id`, `turn_id`) are the hook's own and do NOT match
/// any persisted transcript id, so a live delta cannot be id-joined to the
/// eventually-persisted message — it is reconciled per turn instead.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageDisplayPayload {
    pub session_id: String,
    /// The display message these chunks belong to (stable across one message's
    /// fires; not a transcript id).
    pub message_id: String,
    /// The chunk's position within the message, increasing 0, 1, 2, …
    pub index: u32,
    /// True only on the last chunk of a message.
    #[serde(default)]
    pub r#final: bool,
    /// The visible assistant text chunk.
    pub delta: String,
    /// The per-turn id the hook stamps. Carried for completeness; Delta
    /// attributes the stream to the in-flight turn's thread rather than joining
    /// on this id (it is not a transcript id).
    #[serde(default)]
    pub turn_id: Option<String>,
}
