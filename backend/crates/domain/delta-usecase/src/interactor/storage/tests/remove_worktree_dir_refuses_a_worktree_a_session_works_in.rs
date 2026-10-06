use crate::error::Error;
use crate::interactor::testing::*;

use super::support::{session_in, under_base, REPO_ROOT};

/// A worktree a listed session still works in is that session's: Storage
/// refuses it even forced, and the way to clean it up is removing the session.
#[tokio::test]
async fn remove_worktree_dir_refuses_a_worktree_a_session_works_in() {
    let path = under_base("owned");
    let ix = interactor_with_workspace_and_git(
        FakeWorkspace::default(),
        FakeGitWorktree::default().with_inspection(&path, REPO_ROOT, false),
    );
    session_in(&ix, "sess-1", &path).await;

    let result = ix.remove_worktree_dir(&path, true).await;

    assert!(
        matches!(result, Err(Error::WorktreeInUse(_))),
        "got {result:?}"
    );
    assert!(ix.git_worktree_fake().removal_untouched());
}
