use crate::interactor::testing::*;

use super::support::{now, BASE_PARENT};

/// A worktree base that still holds something — a kept worktree, or a file the
/// user put there — stays, and so does the directory above it.
#[tokio::test]
async fn erase_everything_keeps_a_worktree_base_that_is_not_empty() {
    let workspace = FakeWorkspace::default();
    workspace
        .non_empty_dirs
        .lock()
        .unwrap()
        .push(TEST_WORKTREE_BASE.to_owned());
    let ix = interactor_with_tmux_workspace_and_git(
        FakeTmux::default(),
        workspace,
        FakeGitWorktree::default(),
    );

    let report = ix.erase_everything(now(), Some(BASE_PARENT)).await.unwrap();

    assert_eq!(report, crate::EraseReport::default());
    assert!(ix
        .workspace_fake()
        .removed_empty_dirs
        .lock()
        .unwrap()
        .is_empty());
    assert_eq!(*ix.tmux_fake().servers_killed.lock().unwrap(), 1);
}
