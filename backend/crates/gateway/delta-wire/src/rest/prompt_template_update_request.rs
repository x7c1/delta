//! Request body for `PATCH /api/prompt-templates/{id}`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `PATCH /api/prompt-templates/{id}`.
///
/// Replaces a registered template's content in place, so its id and `created_at`
/// are preserved (a delete+recreate would churn both, and reorder the list) while
/// `updated_at` is re-stamped. Both fields are required — this is a full
/// replacement of the editable content, not a partial patch — and both are held
/// to the same non-blank rule as the create.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "UpdatePromptTemplateRequest")]
pub struct WireUpdatePromptTemplateRequest {
    /// The template's new name in the picker. Required, non-blank.
    pub label: String,
    /// The template's new body. Required, non-blank; stored verbatim.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_fields_are_required_and_round_trip() {
        let req: WireUpdatePromptTemplateRequest =
            serde_json::from_str(r#"{"label":"Final","text":"second wording"}"#).unwrap();
        assert_eq!(req.label, "Final");
        assert_eq!(req.text, "second wording");

        // A partial patch is not a valid body: an edit always carries both
        // fields, so a client cannot blank one by omission.
        assert!(
            serde_json::from_str::<WireUpdatePromptTemplateRequest>(r#"{"label":"Final"}"#)
                .is_err()
        );
    }
}
