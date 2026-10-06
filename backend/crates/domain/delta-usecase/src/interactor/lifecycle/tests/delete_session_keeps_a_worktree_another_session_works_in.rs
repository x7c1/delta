use crate::interactor::testing::*;
use crate::{DiskItem, KeepReason, KeptItem};

use super::removal_support::{closed_session, worktree_path, REPO_ROOT};

/// Two sessions can share one worktree (a second session started on a branch
/// the first already has checked out reuses its worktree). Removing one of
/// them must not pull the working copy out from under the other.
#[tokio::test]
async fn delete_session_keeps_a_worktree_another_session_works_in() {
    let ix = interactor();
    let path = worktree_path("sess-1");
    let id = closed_session(&ix, "sess-1", &path, Some(REPO_ROOT), Some("feature")).await;
    closed_session(&ix, "sess-2", &path, Some(REPO_ROOT), Some("feature")).await;

    let removal = ix.delete_session(&id).await.unwrap();

    assert!(
        removal.removed.is_empty(),
        "nothing was removed, got {removal}"
    );
    assert_eq!(
        removal.kept,
        vec![
            KeptItem {
                item: DiskItem::Worktree(path),
                reason: KeepReason::InUseByAnotherSession,
            },
            KeptItem {
                item: DiskItem::Branch("feature".into()),
                reason: KeepReason::NotCreatedByDelta,
            },
        ]
    );
    assert!(
        ix.git_worktree_fake().removal_untouched(),
        "git is not asked anything"
    );
}
