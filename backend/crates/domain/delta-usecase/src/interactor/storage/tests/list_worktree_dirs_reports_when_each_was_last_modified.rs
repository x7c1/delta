use std::time::{Duration, UNIX_EPOCH};

use crate::interactor::testing::*;

use super::support::under_base;

/// Each directory carries its modification time as an ISO-8601 UTC
/// timestamp, and a directory whose metadata cannot be read is still listed,
/// without one.
#[tokio::test]
async fn list_worktree_dirs_reports_when_each_was_last_modified() {
    let (dated, undated) = (under_base("dated"), under_base("undated"));
    let ix = interactor_with_workspace_and_git(
        FakeWorkspace::default()
            .with_children(TEST_WORKTREE_BASE, &["dated", "undated"])
            // 2026-01-01T00:00:00Z
            .with_modified(&dated, UNIX_EPOCH + Duration::from_secs(1_767_225_600)),
        FakeGitWorktree::default(),
    );

    let dirs = ix.list_worktree_dirs().await.unwrap();

    let modified: Vec<_> = dirs
        .iter()
        .map(|dir| (dir.path.as_str(), dir.modified_at.as_deref()))
        .collect();
    assert_eq!(
        modified,
        [
            (dated.as_str(), Some("2026-01-01T00:00:00Z")),
            (undated.as_str(), None),
        ]
    );
}
