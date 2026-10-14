//! Wire form of the opt-in git-worktree request carried on a `new_session`
//! send.

use delta_usecase::WorktreeSpec;
use serde::Deserialize;
use ts_rs::TS;

use crate::rest::worktree_start_point::WireWorktreeStartPoint;

/// The opt-in worktree request a `new_session` send may carry.
///
/// Wire twin of the domain [`WorktreeSpec`]: when present on a `new_session`
/// send whose selected directory is a git repository, Delta creates a
/// per-session worktree and launches there. `start_point` is where the branch
/// starts; `confirmed_commit` is the commit the user confirmed in the launch
/// confirmation (`POST /api/launch-trust` reported it), omitted when none was
/// asked.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "WorktreeSpec")]
pub struct WireWorktreeSpec {
    pub start_point: WireWorktreeStartPoint,
    #[serde(default)]
    #[ts(optional)]
    pub confirmed_commit: Option<String>,
}

impl From<WireWorktreeSpec> for WorktreeSpec {
    fn from(spec: WireWorktreeSpec) -> Self {
        WorktreeSpec {
            start_point: spec.start_point.into(),
            confirmed_commit: spec.confirmed_commit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_usecase::WorktreeStartPoint;

    #[test]
    fn head_start_point_deserializes_from_its_kind_tag() {
        let spec: WireWorktreeSpec =
            serde_json::from_str(r#"{ "start_point": { "kind": "head" } }"#).unwrap();
        assert_eq!(spec.start_point, WireWorktreeStartPoint::Head);
        assert_eq!(
            WorktreeSpec::from(spec).start_point,
            WorktreeStartPoint::Head
        );
    }

    #[test]
    fn confirmed_commit_is_optional_and_carried_through() {
        let bare: WireWorktreeSpec =
            serde_json::from_str(r#"{ "start_point": { "kind": "head" } }"#).unwrap();
        assert_eq!(WorktreeSpec::from(bare).confirmed_commit, None);

        let confirmed: WireWorktreeSpec = serde_json::from_str(
            r#"{ "start_point": { "kind": "head" }, "confirmed_commit": "abc123" }"#,
        )
        .unwrap();
        assert_eq!(
            WorktreeSpec::from(confirmed).confirmed_commit.as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn remote_branch_start_point_carries_its_name() {
        let spec: WireWorktreeSpec = serde_json::from_str(
            r#"{ "start_point": { "kind": "remote_branch", "name": "main" } }"#,
        )
        .unwrap();
        assert_eq!(
            spec.start_point,
            WireWorktreeStartPoint::RemoteBranch {
                name: "main".to_owned()
            }
        );
        assert_eq!(
            WorktreeSpec::from(spec).start_point,
            WorktreeStartPoint::RemoteBranch("main".to_owned())
        );
    }

    #[test]
    fn use_remote_branch_start_point_carries_its_name() {
        let spec: WireWorktreeSpec = serde_json::from_str(
            r#"{ "start_point": { "kind": "use_remote_branch", "name": "feature/x" } }"#,
        )
        .unwrap();
        assert_eq!(
            spec.start_point,
            WireWorktreeStartPoint::UseRemoteBranch {
                name: "feature/x".to_owned()
            }
        );
        assert_eq!(
            WorktreeSpec::from(spec).start_point,
            WorktreeStartPoint::UseRemoteBranch("feature/x".to_owned())
        );
    }
}
