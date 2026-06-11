//! The subset of a transcript line Delta reads.

use serde::Deserialize;

use super::raw_attachment::RawAttachment;
use super::raw_message::RawMessage;

/// The subset of a transcript line Delta reads. Unknown fields are ignored.
#[derive(Debug, Deserialize)]
pub(super) struct RawLine {
    pub uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    #[serde(rename = "type")]
    pub line_type: Option<String>,
    #[serde(rename = "promptId")]
    pub prompt_id: Option<String>,
    pub timestamp: Option<String>,
    pub message: Option<RawMessage>,
    /// Present on `type: "attachment"` lines; carries a queued command's prompt.
    pub attachment: Option<RawAttachment>,
    /// Set on harness-injected lines (skill bodies, system reminders,
    /// local-command output) that Claude records as `type: "user"` but are not
    /// human-authored turns. Drives [`Role::Meta`] classification.
    #[serde(rename = "isMeta")]
    pub is_meta: Option<bool>,
}
