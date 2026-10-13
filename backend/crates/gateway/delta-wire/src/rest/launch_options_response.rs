//! Response for `GET /api/launch-options`.

use serde::Serialize;
use ts_rs::TS;

use crate::rest::launch_option::WireLaunchOption;

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
    use delta_model::LaunchOption;

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
                    None,
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
                    "choice_group": null,
                }],
            }),
        );
    }
}
