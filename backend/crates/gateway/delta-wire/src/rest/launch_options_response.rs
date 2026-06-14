//! Response for `GET /api/launch-options`.

use delta_usecase::LaunchOption;
use serde::Serialize;
use ts_rs::TS;

/// One registered launch option: a flat `(label?, name, value?)` record for a
/// custom `claude` CLI flag. `name` is the flag (e.g. `--plugin-dir`), `value`
/// its argument (`null` for a valueless flag), and `label` an optional note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "LaunchOption")]
pub struct WireLaunchOption {
    pub id: i64,
    pub label: Option<String>,
    pub name: String,
    pub value: Option<String>,
    pub created_at: String,
}

impl From<LaunchOption> for WireLaunchOption {
    fn from(option: LaunchOption) -> Self {
        WireLaunchOption {
            id: option.id,
            label: option.label,
            name: option.name,
            value: option.value,
            created_at: option.created_at,
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

    #[test]
    fn launch_options_serialize_with_the_rest_field_names() {
        assert_eq!(
            serde_json::to_value(WireLaunchOptionsResponse {
                launch_options: vec![WireLaunchOption::from(LaunchOption {
                    id: 1,
                    label: Some("plugins".to_owned()),
                    name: "--plugin-dir".to_owned(),
                    value: Some("/opt/p".to_owned()),
                    created_at: "2026-01-01T00:00:00Z".to_owned(),
                })],
            })
            .unwrap(),
            serde_json::json!({
                "launch_options": [{
                    "id": 1,
                    "label": "plugins",
                    "name": "--plugin-dir",
                    "value": "/opt/p",
                    "created_at": "2026-01-01T00:00:00Z",
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
                created_at: "2026-01-01T00:00:00Z".to_owned(),
            }))
            .unwrap(),
            serde_json::json!({
                "id": 2,
                "label": null,
                "name": "--dangerously-skip-permissions",
                "value": null,
                "created_at": "2026-01-01T00:00:00Z",
            }),
        );
    }
}
