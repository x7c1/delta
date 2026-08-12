//! The `attachment` field of a transcript line.

use serde::Deserialize;

/// A Claude Code `attachment` payload. Delta only models the `queued_command`
/// variant, which carries a prompt the user queued while a turn was in flight;
/// all other attachment kinds are ignored (they parse with `prompt: None`).
///
/// The `queued_command` shape is legacy-format compatibility: only older
/// claude versions recorded queued prompts this way (current claude writes a
/// uuid-less `queue-operation` line and replays the prompt as a plain user
/// line — see the queued-prompt drift note in docs/guides/development/canary.md).
#[derive(Debug, Deserialize)]
pub(super) struct RawAttachment {
    #[serde(rename = "type")]
    pub attachment_type: Option<String>,
    pub prompt: Option<String>,
}
