use crate::error::Error;
use crate::interactor::testing::*;

use super::support::{under_base, REPO_ROOT};

/// Work is destroyed only when the user said so: a dirty worktree, or a
/// directory git no longer knows (so nothing can say it holds no work), is
/// refused without `force`. With it, the dirty worktree goes through git's
/// forced removal and the unknown directory is deleted as a plain tree.
#[tokio::test]
async fn remove_worktree_dir_needs_force_for_a_dirty_or_unregistered_worktree() {
    let (dirty, gone) = (under_base("dirty"), under_base("gone"));
    let ix = interactor_with_workspace_and_git(
        FakeWorkspace::default(),
        FakeGitWorktree::default().with_inspection(&dirty, REPO_ROOT, true),
    );

    let refused_dirty = ix.remove_worktree_dir(&dirty, false).await;
    let refused_gone = ix.remove_worktree_dir(&gone, false).await;

    assert!(
        matches!(refused_dirty, Err(Error::WorktreeDirty(_))),
        "got {refused_dirty:?}"
    );
    assert!(
        matches!(refused_gone, Err(Error::WorktreeNotRegistered(_))),
        "got {refused_gone:?}"
    );
    assert!(ix.git_worktree_fake().removal_untouched());
    assert!(ix.workspace_fake().removed_trees.lock().unwrap().is_empty());

    ix.remove_worktree_dir(&dirty, true).await.unwrap();
    ix.remove_worktree_dir(&gone, true).await.unwrap();

    let git = ix.git_worktree_fake();
    assert_eq!(
        *git.force_removed.lock().unwrap(),
        vec![(REPO_ROOT.to_owned(), dirty.clone())]
    );
    assert!(
        git.removed_worktrees.lock().unwrap().is_empty(),
        "a worktree known to be dirty skips the removal git would refuse"
    );
    assert_eq!(
        *ix.workspace_fake().removed_trees.lock().unwrap(),
        vec![gone.clone()]
    );
    assert_eq!(
        *git.pruned.lock().unwrap(),
        vec![REPO_ROOT.to_owned()],
        "only the worktree whose repository git named is pruned"
    );
    assert_eq!(*git.forgotten.lock().unwrap(), vec![dirty, gone]);
}
