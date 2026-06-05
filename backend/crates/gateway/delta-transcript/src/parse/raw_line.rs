//! The subset of a transcript line Delta reads.

use serde::Deserialize;

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
}
