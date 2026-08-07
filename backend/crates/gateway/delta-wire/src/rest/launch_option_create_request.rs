//! Request body for `POST /api/launch-options`.

use serde::Deserialize;
use ts_rs::TS;

use crate::session::WireAgentProvider;

/// Request body for `POST /api/launch-options`.
///
/// Registers one custom agent startup setting as a flat `(label?, name,
/// value?)` record. `name` is required and is read in the vocabulary of the
/// option's `provider` — a CLI flag for Claude (`--plugin-dir`), a
/// `thread/start` field for Codex (`model`); `value` is its argument/value
/// (e.g. `/path/to/plugins`, `gpt-5-codex`) and is omitted for a valueless
/// option; `label` is an optional human-friendly note. The optional fields
/// default to absent, matching what serde accepts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "CreateLaunchOptionRequest")]
pub struct WireCreateLaunchOptionRequest {
    /// An optional human-friendly note for the row.
    #[serde(default)]
    #[ts(optional)]
    pub label: Option<String>,
    /// What the option is called in the provider's vocabulary, e.g.
    /// `--plugin-dir` (Claude) or `model` (Codex). Required.
    pub name: String,
    /// The option's argument/value, e.g. `/path/to/plugins`. Omitted for a
    /// valueless option.
    #[serde(default)]
    #[ts(optional)]
    pub value: Option<String>,
    /// Whether the option starts pre-checked in the session-start picker.
    /// Defaults to `false` (off) when omitted.
    #[serde(default)]
    pub default_enabled: bool,
    /// The provider this option applies to. Omitted defaults to Claude for
    /// back-compat with clients that predate per-provider launch options; the
    /// handler resolves the absent case to `AgentProvider::Claude`.
    #[serde(default)]
    #[ts(optional)]
    pub provider: Option<WireAgentProvider>,
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
        assert!(!req.default_enabled);
        // Omitted `provider` deserializes as absent; the handler resolves it to
        // Claude for back-compat.
        assert_eq!(req.provider, None);

        let req: WireCreateLaunchOptionRequest = serde_json::from_str(
            r#"{"label":"plugins","name":"--plugin-dir","value":"/opt/p","default_enabled":true}"#,
        )
        .unwrap();
        assert_eq!(req.label.as_deref(), Some("plugins"));
        assert_eq!(req.value.as_deref(), Some("/opt/p"));
        assert!(req.default_enabled);
    }

    #[test]
    fn an_explicit_provider_is_deserialized() {
        let req: WireCreateLaunchOptionRequest =
            serde_json::from_str(r#"{"name":"model","value":"gpt-5","provider":"codex"}"#).unwrap();
        assert_eq!(req.provider, Some(WireAgentProvider::Codex));
    }
}
