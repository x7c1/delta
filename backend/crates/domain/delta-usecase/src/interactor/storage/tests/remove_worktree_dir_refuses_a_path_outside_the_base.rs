use crate::error::Error;
use crate::interactor::testing::*;

/// Only a directory directly under the worktree base is removable from
/// Storage: anything else is refused before git or the filesystem is asked
/// anything, forced or not.
#[tokio::test]
async fn remove_worktree_dir_refuses_a_path_outside_the_base() {
    let ix =
        interactor_with_workspace_and_git(FakeWorkspace::default(), FakeGitWorktree::default());

    for path in [
        "/home/u/project".to_owned(),
        TEST_WORKTREE_BASE.to_owned(),
        format!("{TEST_WORKTREE_BASE}/a/b"),
        format!("{TEST_WORKTREE_BASE}/../etc"),
    ] {
        let result = ix.remove_worktree_dir(&path, true).await;
        assert!(
            matches!(result, Err(Error::WorktreeOutsideBase(_))),
            "{path} is refused, got {result:?}"
        );
    }
    assert!(ix.git_worktree_fake().removal_untouched());
    assert!(ix.workspace_fake().removed_trees.lock().unwrap().is_empty());
}
