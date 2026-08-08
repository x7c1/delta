//! Response for `GET /api/launch-options`.

use delta_usecase::LaunchOption;
use serde::Serialize;
use ts_rs::TS;

use crate::session::WireAgentProvider;

/// One registered launch option: a flat `(label?, name, value?)` record for a
/// custom agent startup setting. `name` and `value` are read in the provider's
/// own vocabulary — a CLI flag and its argument for Claude (`--plugin-dir`), a
/// `thread/start` field and its value for Codex (`model`) — with `value` null
/// for a valueless option, and `label` an optional note.
/// `default_enabled` marks it to start pre-checked in the session-start picker.
/// `provider` is the provider the option applies to; the session-start picker
/// only offers options matching the new session's provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "LaunchOption")]
pub struct WireLaunchOption {
    pub id: i64,
    pub label: Option<String>,
    pub name: String,
    pub value: Option<String>,
    pub default_enabled: bool,
    pub created_at: String,
    pub provider: WireAgentProvider,
}

impl From<LaunchOption> for WireLaunchOption {
    fn from(option: LaunchOption) -> Self {
        WireLaunchOption {
            id: option.id,
            label: option.label,
            name: option.name,
            value: option.value,
            default_enabled: option.default_enabled,
            created_at: option.created_at,
            provider: option.provider.into(),
        }
    }
}

/// Response for `GET /api/launch-options`: the registered launch options,
/// newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "LaunchOptionsResponse")]
pub struct WireLaunchOptionsResponse {
    pub launch_options: Vec<WireLaunchOption>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use delta_model::AgentProvider;

    #[test]
    fn launch_options_serialize_with_the_rest_field_names() {
        assert_eq!(
            serde_json::to_value(WireLaunchOptionsResponse {
                launch_options: vec![WireLaunchOption::from(LaunchOption {
                    id: 1,
                    label: Some("plugins".to_owned()),
                    name: "--plugin-dir".to_owned(),
                    value: Some("/opt/p".to_owned()),
                    default_enabled: true,
                    created_at: "2026-01-01T00:00:00Z".to_owned(),
                    provider: AgentProvider::Claude,
                })],
            })
            .unwrap(),
            serde_json::json!({
                "launch_options": [{
                    "id": 1,
                    "label": "plugins",
                    "name": "--plugin-dir",
                    "value": "/opt/p",
                    "default_enabled": true,
                    "created_at": "2026-01-01T00:00:00Z",
                    "provider": "claude",
                }],
            }),
        );
    }

    #[test]
    fn a_valueless_flag_serializes_null_label_and_value() {
        assert_eq!(
            serde_json::to_value(WireLaunchOption::from(LaunchOption {
                id: 2,
                label: None,
                name: "--dangerously-skip-permissions".to_owned(),
                value: None,
                default_enabled: false,
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                provider: AgentProvider::Claude,
            }))
            .unwrap(),
            serde_json::json!({
                "id": 2,
                "label": null,
                "name": "--dangerously-skip-permissions",
                "value": null,
                "default_enabled": false,
                "created_at": "2026-01-01T00:00:00Z",
                "provider": "claude",
            }),
        );
    }

    #[test]
    fn a_codex_option_serializes_its_provider_token() {
        assert_eq!(
            serde_json::to_value(WireLaunchOption::from(LaunchOption {
                id: 3,
                label: None,
                name: "model".to_owned(),
                value: Some("gpt-5".to_owned()),
                default_enabled: false,
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                provider: AgentProvider::Codex,
            }))
            .unwrap()["provider"],
            serde_json::json!("codex"),
        );
    }
}
