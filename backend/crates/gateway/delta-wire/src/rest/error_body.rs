//! The JSON body returned for any error response.

use serde::Serialize;
use ts_rs::TS;

/// The JSON body returned for any error response.
///
/// `error` is a human-readable message. `code` is an optional stable,
/// machine-readable identifier the frontend can branch on (e.g.
/// `"resume_unavailable"`); it is omitted from the JSON entirely for errors
/// that carry no distinct code, so those responses stay unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "ErrorBody")]
pub struct WireErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_omitted_when_absent() {
        assert_eq!(
            serde_json::to_value(WireErrorBody {
                error: "boom".into(),
                code: None,
            })
            .unwrap(),
            serde_json::json!({ "error": "boom" }),
        );
        assert_eq!(
            serde_json::to_value(WireErrorBody {
                error: "cannot resume".into(),
                code: Some("resume_unavailable".into()),
            })
            .unwrap(),
            serde_json::json!({ "error": "cannot resume", "code": "resume_unavailable" }),
        );
    }
}
