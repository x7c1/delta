//! The embedded `message` object of a transcript line.

use serde::Deserialize;

use super::raw_content::RawContent;

#[derive(Debug, Deserialize)]
pub(super) struct RawMessage {
    pub content: Option<RawContent>,
    /// The model that produced this message (present on assistant lines), e.g.
    /// `claude-opus-4-8`. Absent on user/system lines.
    pub model: Option<String>,
}
