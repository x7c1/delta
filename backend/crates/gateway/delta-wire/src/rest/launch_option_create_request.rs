//! Request body for `POST /api/launch-options`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `POST /api/launch-options`.
///
/// Registers one custom `claude` CLI flag as a flat `(label?, name, value?)`
/// record. `name` is the flag (e.g. `--plugin-dir`, `--permission-mode`) and is
/// required; `value` is its argument (e.g. `/path/to/plugins`, `auto`) and is
/// omitted for a valueless flag; `label` is an optional human-friendly note.
/// The optional fields default to absent, matching what serde accepts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "CreateLaunchOptionRequest")]
pub struct WireCreateLaunchOptionRequest {
    /// An optional human-friendly note for the row.
    #[serde(default)]
    #[ts(optional)]
    pub label: Option<String>,
    /// The flag itself, e.g. `--plugin-dir`. Required.
    pub name: String,
    /// The flag's argument, e.g. `/path/to/plugins`. Omitted for a valueless flag.
    #[serde(default)]
    #[ts(optional)]
    pub value: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_optional_fields_default_on_deserialize() {
        let req: WireCreateLaunchOptionRequest =
            serde_json::from_str(r#"{"name":"--dangerously-skip-permissions"}"#).unwrap();
        assert_eq!(req.name, "--dangerously-skip-permissions");
        assert_eq!(req.label, None);
        assert_eq!(req.value, None);

        let req: WireCreateLaunchOptionRequest = serde_json::from_str(
            r#"{"label":"plugins","name":"--plugin-dir","value":"/opt/p"}"#,
        )
        .unwrap();
        assert_eq!(req.label.as_deref(), Some("plugins"));
        assert_eq!(req.value.as_deref(), Some("/opt/p"));
    }
}
