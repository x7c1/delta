//! Request body for `POST /api/prompt-templates`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `POST /api/prompt-templates`.
///
/// Registers one named block of instruction text. Both fields are required and
/// must be non-blank (blank meaning empty once trimmed); `text` is stored
/// verbatim, so its own leading/trailing whitespace and newlines survive — the
/// trim decides only whether the request is acceptable, never what is stored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "CreatePromptTemplateRequest")]
pub struct WireCreatePromptTemplateRequest {
    /// What the template is called in the picker. Required, non-blank.
    pub label: String,
    /// The text inserted into the composer. Required, non-blank.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_fields_are_required_and_round_trip() {
        let req: WireCreatePromptTemplateRequest =
            serde_json::from_str(r#"{"label":"Merge","text":"Once CI is green, merge."}"#).unwrap();
        assert_eq!(req.label, "Merge");
        assert_eq!(req.text, "Once CI is green, merge.");

        // A body missing either field is rejected by serde itself, before any
        // handler validation runs.
        assert!(
            serde_json::from_str::<WireCreatePromptTemplateRequest>(r#"{"label":"Merge"}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<WireCreatePromptTemplateRequest>(r#"{"text":"body"}"#).is_err()
        );
    }

    /// Newlines and surrounding whitespace in `text` survive deserialization
    /// untouched — the server stores what the user wrote.
    #[test]
    fn the_text_keeps_its_newlines() {
        let req: WireCreatePromptTemplateRequest =
            serde_json::from_str("{\"label\":\"Multi\",\"text\":\"\\nfirst\\nsecond\\n\"}")
                .unwrap();
        assert_eq!(req.text, "\nfirst\nsecond\n");
    }
}
