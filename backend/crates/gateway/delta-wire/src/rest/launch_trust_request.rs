//! Request body for `POST /api/launch-trust`.

use delta_model::AgentProvider;
use delta_usecase::WorktreeStartPoint;
use serde::Deserialize;
use ts_rs::TS;

use crate::agent_provider::WireAgentProvider;
use crate::rest::worktree_start_point::WireWorktreeStartPoint;

/// Request body for `POST /api/launch-trust`: the new-session launch about to
/// be sent — the selected directory, where its worktree's branch starts, and
/// the provider — so the server can say whether it needs the user's
/// confirmation first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "LaunchTrustRequest")]
pub struct WireLaunchTrustRequest {
    /// The directory the user selected (a git repository).
    pub workdir: String,
    /// Where the worktree's branch starts from, exactly as the send's
    /// `worktree.start_point` will carry it.
    pub start_point: WireWorktreeStartPoint,
    /// The provider the session will launch on. Omitted means Claude, as on
    /// the send itself.
    #[serde(default)]
    #[ts(optional)]
    pub provider: Option<WireAgentProvider>,
}

impl WireLaunchTrustRequest {
    /// The domain start point and provider (an omitted provider is Claude).
    pub fn into_parts(self) -> (String, WorktreeStartPoint, AgentProvider) {
        (
            self.workdir,
            self.start_point.into(),
            self.provider
                .map_or(AgentProvider::Claude, AgentProvider::from),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_omitted_provider_is_claude() {
        let request: WireLaunchTrustRequest = serde_json::from_str(
            r#"{ "workdir": "/r", "start_point": { "kind": "remote_branch", "name": "x" } }"#,
        )
        .unwrap();
        assert_eq!(
            request.into_parts(),
            (
                "/r".to_owned(),
                WorktreeStartPoint::RemoteBranch("x".to_owned()),
                AgentProvider::Claude
            )
        );
    }
}
