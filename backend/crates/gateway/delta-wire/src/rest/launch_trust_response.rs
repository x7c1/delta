//! Response for `POST /api/launch-trust`.

use delta_usecase::{AgentConfigChange, LaunchTrustCheck};
use serde::Serialize;
use ts_rs::TS;

/// One agent-configuration path that differs between the trusted repository
/// `HEAD` and the commit the worktree would launch on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "AgentConfigChange")]
pub struct WireAgentConfigChange {
    /// The path, relative to the repository top.
    pub path: String,
    /// The unified diff of the path, as git renders it. Empty when omitted.
    pub diff: String,
    /// Whether `diff` was cut short or omitted because it was too large.
    pub diff_truncated: bool,
}

impl From<AgentConfigChange> for WireAgentConfigChange {
    fn from(change: AgentConfigChange) -> Self {
        Self {
            path: change.path,
            diff: change.diff,
            diff_truncated: change.diff_truncated,
        }
    }
}

/// Response for `POST /api/launch-trust`: whether the launch needs the user's
/// confirmation before Claude Code's trust is pre-accepted for its worktree,
/// why, and what to show.
///
/// When `needs_confirmation` is true the send must carry `commit` as
/// `worktree.confirmed_commit` once the user accepts; a launch whose worktree
/// ends up on any other commit is not pre-accepted and fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "LaunchTrustResponse")]
pub struct WireLaunchTrustResponse {
    pub needs_confirmation: bool,
    /// The repository has not been trusted in Claude Code yet.
    pub repository_untrusted: bool,
    /// The commit the worktree would launch on; `null` when the launch
    /// pre-accepts nothing (nothing to confirm).
    pub commit: Option<String>,
    /// The agent-configuration paths the branch changes, with their diffs.
    pub agent_config_changes: Vec<WireAgentConfigChange>,
}

impl From<LaunchTrustCheck> for WireLaunchTrustResponse {
    fn from(check: LaunchTrustCheck) -> Self {
        Self {
            needs_confirmation: check.needs_confirmation(),
            repository_untrusted: check.repository_untrusted,
            commit: check.commit,
            agent_config_changes: check
                .agent_config_changes
                .into_iter()
                .map(WireAgentConfigChange::from)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_serializes_with_its_reasons_and_diffs() {
        let check = LaunchTrustCheck {
            repository_untrusted: false,
            commit: Some("abc".into()),
            agent_config_changes: vec![AgentConfigChange {
                path: ".claude/settings.json".into(),
                diff: "@@\n-a\n+b".into(),
                diff_truncated: false,
            }],
        };
        assert_eq!(
            serde_json::to_value(WireLaunchTrustResponse::from(check)).unwrap(),
            serde_json::json!({
                "needs_confirmation": true,
                "repository_untrusted": false,
                "commit": "abc",
                "agent_config_changes": [
                    { "path": ".claude/settings.json", "diff": "@@\n-a\n+b", "diff_truncated": false }
                ],
            }),
        );
    }
}
