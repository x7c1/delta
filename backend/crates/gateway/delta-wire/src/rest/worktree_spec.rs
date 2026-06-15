//! Wire form of the opt-in git-worktree request carried on a `new_session`
//! send.

use delta_usecase::{WorktreeSpec, WorktreeStartPoint};
use serde::Deserialize;
use ts_rs::TS;

/// Where a new session's worktree branch should start from.
///
/// Wire twin of the domain [`WorktreeStartPoint`], internally tagged by `kind`
/// (mirroring the `/ws` event union): `{ "kind": "head" }` branches off the
/// repository's current `HEAD`, and `{ "kind": "remote_branch", "name": "..." }`
/// branches off the named remote branch (fetched first).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(rename = "WorktreeStartPoint")]
pub enum WireWorktreeStartPoint {
    /// Branch off the repository's current `HEAD`.
    Head,
    /// Branch off `origin/<name>`, fetched first. `name` is the remote branch
    /// short name (no `origin/` prefix).
    RemoteBranch { name: String },
}

impl From<WireWorktreeStartPoint> for WorktreeStartPoint {
    fn from(start_point: WireWorktreeStartPoint) -> Self {
        match start_point {
            WireWorktreeStartPoint::Head => WorktreeStartPoint::Head,
            WireWorktreeStartPoint::RemoteBranch { name } => WorktreeStartPoint::RemoteBranch(name),
        }
    }
}

/// The opt-in worktree request a `new_session` send may carry.
///
/// Wire twin of the domain [`WorktreeSpec`]: when present on a `new_session`
/// send whose selected directory is a git repository, Delta creates a
/// per-session worktree and launches there. The only knob is the branch start
/// point.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "WorktreeSpec")]
pub struct WireWorktreeSpec {
    pub start_point: WireWorktreeStartPoint,
}

impl From<WireWorktreeSpec> for WorktreeSpec {
    fn from(spec: WireWorktreeSpec) -> Self {
        WorktreeSpec {
            start_point: spec.start_point.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
