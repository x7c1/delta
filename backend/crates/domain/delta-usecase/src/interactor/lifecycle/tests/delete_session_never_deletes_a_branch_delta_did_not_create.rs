use crate::interactor::testing::*;
use crate::{DiskItem, KeepReason, KeptItem};

use super::removal_support::{closed_session, worktree_path, REPO_ROOT};

/// A session started on an existing branch (a pull request's, say) worked in
/// a worktree Delta created, so the clean worktree goes — but the branch is
/// the user's, merged or not, and git is never asked to delete it.
#[tokio::test]
async fn delete_session_never_deletes_a_branch_delta_did_not_create() {
    let ix = interactor();
    let path = worktree_path("sess-1");
    let id = closed_session(&ix, "sess-1", &path, Some(REPO_ROOT), Some("feature/login")).await;

    let removal = ix.delete_session(&id).await.unwrap();

    assert_eq!(
        removal.removed,
        vec![DiskItem::Worktree(path.clone()), DiskItem::TrustEntry(path)]
    );
    assert_eq!(
        removal.kept,
        vec![KeptItem {
            item: DiskItem::Branch("feature/login".into()),
            reason: KeepReason::NotCreatedByDelta,
        }]
    );
    assert!(
        ix.git_worktree_fake()
            .deleted_branches
            .lock()
            .unwrap()
            .is_empty(),
        "no branch deletion is attempted"
    );
}
