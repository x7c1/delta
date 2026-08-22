//! Response for `GET /api/prompt-templates`.

use delta_usecase::PromptTemplate;
use serde::Serialize;
use ts_rs::TS;

/// One registered prompt template: a named block of instruction text the user
/// inserts into the composer at the cursor. `label` names it in the picker;
/// `text` is inserted verbatim, whitespace and newlines included. There is no
/// `provider` — the text is prose the agent reads, so the same template applies
/// to every provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PromptTemplate")]
pub struct WirePromptTemplate {
    pub id: i64,
    pub label: String,
    pub text: String,
    pub created_at: String,
    /// When the content was last edited; equal to `created_at` until the first
    /// edit.
    pub updated_at: String,
}

impl From<PromptTemplate> for WirePromptTemplate {
    fn from(template: PromptTemplate) -> Self {
        WirePromptTemplate {
            id: template.id,
            label: template.label,
            text: template.text,
            created_at: template.created_at,
            updated_at: template.updated_at,
        }
    }
}

/// Response for `GET /api/prompt-templates`: the registered templates, oldest
/// first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PromptTemplatesResponse")]
pub struct WirePromptTemplatesResponse {
    pub prompt_templates: Vec<WirePromptTemplate>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_templates_serialize_with_the_rest_field_names() {
        assert_eq!(
            serde_json::to_value(WirePromptTemplatesResponse {
                prompt_templates: vec![WirePromptTemplate::from(PromptTemplate {
                    id: 1,
                    label: "Merge and log".to_owned(),
                    text: "Once CI is green, merge.".to_owned(),
                    created_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-02T00:00:00Z".to_owned(),
                })],
            })
            .unwrap(),
            serde_json::json!({
                "prompt_templates": [{
                    "id": 1,
                    "label": "Merge and log",
                    "text": "Once CI is green, merge.",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-02T00:00:00Z",
                }],
            }),
        );
    }

    /// The text crosses the wire byte for byte: a template that ends with a
    /// newline is a template that ends with a newline on the browser side too.
    #[test]
    fn the_text_is_serialized_verbatim() {
        assert_eq!(
            serde_json::to_value(WirePromptTemplate::from(PromptTemplate {
                id: 2,
                label: "Multi".to_owned(),
                text: "\nfirst\nsecond\n".to_owned(),
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                updated_at: "2026-01-01T00:00:00Z".to_owned(),
            }))
            .unwrap()["text"],
            serde_json::json!("\nfirst\nsecond\n"),
        );
    }
}
