use crate::interactor::testing::*;
use crate::ports::WorktreeRemoval;
use crate::{DiskItem, KeepReason, KeptItem};

use super::removal_support::{closed_session, worktree_path, REPO_ROOT};

/// A worktree with uncommitted work is the user's work: git refuses to remove
/// it, Delta keeps it — and the branch checked out in it, which is not even
/// asked about — and says so. The session's row goes regardless: a kept
/// worktree never refuses the removal.
#[tokio::test]
async fn delete_session_keeps_a_dirty_worktree_and_its_branch() {
    let path = worktree_path("sess-1");
    let ix = interactor_with_git(
        FakeGitWorktree::default()
            .with_worktree_removal(&path, Scripted::Answer(WorktreeRemoval::KeptDirty)),
    );
    let id = closed_session(&ix, "sess-1", &path, Some(REPO_ROOT), Some("delta-sess-1")).await;

    let removal = ix
        .delete_session(&id)
        .await
        .expect("a dirty worktree does not refuse the removal");

    assert!(
        ix.store().session(&id).await.unwrap().is_none(),
        "the row is still deleted"
    );
    assert!(
        removal.removed.is_empty(),
        "nothing was removed, got {removal}"
    );
    assert_eq!(
        removal.kept,
        vec![
            KeptItem {
                item: DiskItem::Worktree(path),
                reason: KeepReason::Dirty,
            },
            KeptItem {
                item: DiskItem::Branch("delta-sess-1".into()),
                reason: KeepReason::WorktreeKept,
            },
        ]
    );
    let git = ix.git_worktree_fake();
    assert!(
        git.deleted_branches.lock().unwrap().is_empty(),
        "the branch is left alone"
    );
    assert!(git.pruned.lock().unwrap().is_empty());
    assert!(
        git.forgotten.lock().unwrap().is_empty(),
        "the kept worktree stays trusted"
    );
}
