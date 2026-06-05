//! `UserPromptSubmit` response.

use serde::Serialize;

/// `UserPromptSubmit` response. When present, `additional_context` is injected
/// into this prompt only.
#[derive(Debug, Default, Serialize)]
pub struct UserPromptSubmitResponse {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}
