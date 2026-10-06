use crate::error::Error;
use crate::interactor::testing::*;
use crate::ports::WorktreeRemoval;

use super::support::{under_base, REPO_ROOT};

/// A clean leftover goes the way a removed session's clean worktree goes —
/// git's unforced removal, then prune and the trust entry. One that gained
/// work after it was listed is caught by git's own refusal and refused like a
/// dirty one.
#[tokio::test]
async fn remove_worktree_dir_removes_a_clean_worktree_and_cleans_up() {
    let (clean, raced) = (under_base("clean"), under_base("raced"));
    let ix = interactor_with_workspace_and_git(
        FakeWorkspace::default(),
        FakeGitWorktree::default()
            .with_inspection(&clean, REPO_ROOT, false)
            .with_inspection(&raced, REPO_ROOT, false)
            .with_worktree_removal(&raced, Scripted::Answer(WorktreeRemoval::KeptDirty)),
    );

    ix.remove_worktree_dir(&clean, false).await.unwrap();
    let refused = ix.remove_worktree_dir(&raced, false).await;

    assert!(
        matches!(refused, Err(Error::WorktreeDirty(_))),
        "got {refused:?}"
    );
    let git = ix.git_worktree_fake();
    assert_eq!(
        *git.removed_worktrees.lock().unwrap(),
        vec![
            (REPO_ROOT.to_owned(), clean.clone()),
            (REPO_ROOT.to_owned(), raced),
        ]
    );
    assert!(git.force_removed.lock().unwrap().is_empty());
    assert_eq!(*git.pruned.lock().unwrap(), vec![REPO_ROOT.to_owned()]);
    assert_eq!(*git.forgotten.lock().unwrap(), vec![clean]);
}
