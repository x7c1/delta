//! Response for `GET /api/prompt-templates`.

use serde::Serialize;
use ts_rs::TS;

use crate::rest::prompt_template::WirePromptTemplate;

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

    use delta_model::PromptTemplate;

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
}
