use crate::interactor::testing::*;
use crate::ports::BranchDeletion;
use crate::{DiskItem, KeepReason, KeptItem};

use super::removal_support::{closed_session, worktree_path, REPO_ROOT};

/// Commits that never reached the branch's upstream are the user's work too:
/// the clean worktree goes, but git refuses to delete the unmerged
/// `delta-<id>` branch, and Delta keeps it and says so.
#[tokio::test]
async fn delete_session_keeps_an_unmerged_branch() {
    let ix = interactor_with_git(FakeGitWorktree::default().with_branch_deletion(
        "delta-sess-1",
        Scripted::Answer(BranchDeletion::KeptUnmerged),
    ));
    let path = worktree_path("sess-1");
    let id = closed_session(&ix, "sess-1", &path, Some(REPO_ROOT), Some("delta-sess-1")).await;

    let removal = ix.delete_session(&id).await.unwrap();

    assert_eq!(
        removal.removed,
        vec![DiskItem::Worktree(path.clone()), DiskItem::TrustEntry(path)]
    );
    assert_eq!(
        removal.kept,
        vec![KeptItem {
            item: DiskItem::Branch("delta-sess-1".into()),
            reason: KeepReason::Unmerged,
        }]
    );
}
