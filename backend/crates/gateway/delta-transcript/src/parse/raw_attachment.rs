//! The `attachment` field of a transcript line.

use serde::Deserialize;

/// A Claude Code `attachment` payload. Delta only models the `queued_command`
/// variant, which carries a prompt the user queued while a turn was in flight;
/// all other attachment kinds are ignored (they parse with `prompt: None`).
#[derive(Debug, Deserialize)]
pub(super) struct RawAttachment {
    #[serde(rename = "type")]
    pub attachment_type: Option<String>,
    pub prompt: Option<String>,
}
