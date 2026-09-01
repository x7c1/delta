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
/// `builtin` marks a row Delta ships rather than one the user registered, and
/// `dangerous` a row that disables the agent's own safety mechanism.
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
    /// Whether Delta ships this option (as opposed to the user having
    /// registered it). A shipped option's `label`, `name` and `value` come from
    /// Delta's declared catalog and cannot be edited or deleted — `DELETE`
    /// answers `409` — while `default_enabled` stays the user's to set.
    ///
    /// Deliberately a boolean rather than the internal catalog key: the only
    /// thing a client does with this is badge the row and drop its delete
    /// control, so exposing the key would be a contract nobody needs.
    pub builtin: bool,
    /// Whether this option switches the agent's own safety mechanism off —
    /// Claude's `--dangerously-skip-permissions`, a Codex `danger-full-access`
    /// sandbox, and the handful of other spellings that mean "stop asking".
    ///
    /// **Derived, never stored**: the answer is a function of `(provider, name,
    /// value)` read in the provider's own vocabulary, so it is computed per
    /// response from the gateway that owns that vocabulary rather than persisted
    /// beside the row — a row registered before a spelling was recognised is
    /// marked the moment the predicate learns it, with no migration.
    ///
    /// Such an option stays selectable per session; what it may never be is
    /// silent. The server refuses to *set* `default_enabled` on it (`400`
    /// `launch_option_rejected`) while always accepting it being cleared, so a
    /// client is expected to mark the row and never pre-check it — including when
    /// a row registered before that rule still says `default_enabled: true` —
    /// and to offer the default control only as the way to clear such a stale
    /// flag.
    pub dangerous: bool,
}

impl WireLaunchOption {
    /// Render a registered option for the wire, with the danger verdict the
    /// caller has resolved through the provider's predicate.
    ///
    /// Not a [`From`] impl: `dangerous` cannot be derived from a [`LaunchOption`]
    /// alone — the vocabulary that decides it lives in the gateway layer, and the
    /// use case reads it through a port — so the flag has to be passed in. Making
    /// it an argument rather than a defaulted field means a new call site has to
    /// answer the question instead of silently rendering `false`.
    pub fn new(option: LaunchOption, dangerous: bool) -> Self {
        WireLaunchOption {
            id: option.id,
            label: option.label,
            name: option.name,
            value: option.value,
            default_enabled: option.default_enabled,
            created_at: option.created_at,
            provider: option.provider.into(),
            builtin: option.builtin_key.is_some(),
            dangerous,
        }
    }
}

/// Response for `GET /api/launch-options`: the registered launch options, the
/// rows Delta ships first, then the user's own newest first.
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
                launch_options: vec![WireLaunchOption::new(
                    LaunchOption {
                        id: 1,
                        label: Some("plugins".to_owned()),
                        name: "--plugin-dir".to_owned(),
                        value: Some("/opt/p".to_owned()),
                        default_enabled: true,
                        created_at: "2026-01-01T00:00:00Z".to_owned(),
                        provider: AgentProvider::Claude,
                        builtin_key: None,
                    },
                    false,
                )],
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
                    "builtin": false,
                    "dangerous": false,
                }],
            }),
        );
    }

    /// A valueless flag serializes null `label`/`value` — and this one is the
    /// dangerous flag, so it also pins that the verdict the caller resolved rides
    /// out on the row rather than being re-derived (or lost) here.
    #[test]
    fn a_valueless_flag_serializes_null_label_and_value() {
        assert_eq!(
            serde_json::to_value(WireLaunchOption::new(
                LaunchOption {
                    id: 2,
                    label: None,
                    name: "--dangerously-skip-permissions".to_owned(),
                    value: None,
                    default_enabled: false,
                    created_at: "2026-01-01T00:00:00Z".to_owned(),
                    provider: AgentProvider::Claude,
                    builtin_key: None,
                },
                true,
            ))
            .unwrap(),
            serde_json::json!({
                "id": 2,
                "label": null,
                "name": "--dangerously-skip-permissions",
                "value": null,
                "default_enabled": false,
                "created_at": "2026-01-01T00:00:00Z",
                "provider": "claude",
                "builtin": false,
                "dangerous": true,
            }),
        );
    }

    #[test]
    fn a_codex_option_serializes_its_provider_token() {
        assert_eq!(
            serde_json::to_value(WireLaunchOption::new(
                LaunchOption {
                    id: 3,
                    label: None,
                    name: "model".to_owned(),
                    value: Some("gpt-5".to_owned()),
                    default_enabled: false,
                    created_at: "2026-01-01T00:00:00Z".to_owned(),
                    provider: AgentProvider::Codex,
                    builtin_key: None,
                },
                false,
            ))
            .unwrap()["provider"],
            serde_json::json!("codex"),
        );
    }

    /// A row Delta ships serializes `builtin: true` — and its catalog *key*
    /// never reaches the wire, so the browser learns only that the row is
    /// Delta's own.
    #[test]
    fn a_shipped_option_serializes_builtin_true_without_its_key() {
        let body = serde_json::to_value(WireLaunchOption::new(
            LaunchOption {
                id: 4,
                label: Some("Opus".to_owned()),
                name: "--model".to_owned(),
                value: Some("opus".to_owned()),
                default_enabled: false,
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                provider: AgentProvider::Claude,
                builtin_key: Some("claude:model-opus".to_owned()),
            },
            false,
        ))
        .unwrap();
        assert_eq!(body["builtin"], serde_json::json!(true));
        assert!(
            body.as_object().unwrap().get("builtin_key").is_none(),
            "the catalog key is internal and must not reach the wire: {body}"
        );
    }
}
