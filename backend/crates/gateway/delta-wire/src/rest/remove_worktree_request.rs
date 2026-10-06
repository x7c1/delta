//! Request body for `DELETE /api/storage/worktrees`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `DELETE /api/storage/worktrees`: remove one directory
/// under the worktree base that no listed session works in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "RemoveWorktreeRequest")]
pub struct WireRemoveWorktreeRequest {
    /// The directory, spelled as `GET /api/storage/worktrees` lists it.
    pub path: String,
    /// Remove it even when it has uncommitted or untracked changes, or when
    /// git no longer knows it as a worktree — destroying what is in it.
    /// Absent means `false`.
    #[serde(default)]
    #[ts(optional)]
    pub force: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_is_optional() {
        let request: WireRemoveWorktreeRequest =
            serde_json::from_str(r#"{ "path": "/w/a" }"#).unwrap();
        assert_eq!(request.force, None);
        let request: WireRemoveWorktreeRequest =
            serde_json::from_str(r#"{ "path": "/w/a", "force": true }"#).unwrap();
        assert_eq!(request.force, Some(true));
    }
}
