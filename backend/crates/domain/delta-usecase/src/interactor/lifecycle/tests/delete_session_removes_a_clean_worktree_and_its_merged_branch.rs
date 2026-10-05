use crate::interactor::testing::*;
use crate::{DiskItem, SessionRemoval};

use super::removal_support::{closed_session, worktree_path, REPO_ROOT};

/// Removing a session that ran in a clean worktree Delta created, on the
/// merged `delta-<id>` branch Delta cut for it, removes both: the worktree,
/// then git's record of it (prune) and the trust entry Delta seeded for the
/// path, then the branch — which git only lets go once no worktree has it
/// checked out.
#[tokio::test]
async fn delete_session_removes_a_clean_worktree_and_its_merged_branch() {
    let ix = interactor();
    let path = worktree_path("sess-1");
    let id = closed_session(&ix, "sess-1", &path, Some(REPO_ROOT), Some("delta-sess-1")).await;

    let removal = ix
        .delete_session(&id)
        .await
        .expect("a closed session can be removed");

    assert!(
        ix.store().session(&id).await.unwrap().is_none(),
        "the row is gone"
    );
    assert_eq!(
        removal,
        SessionRemoval {
            removed: vec![
                DiskItem::Worktree(path.clone()),
                DiskItem::TrustEntry(path.clone()),
                DiskItem::Branch("delta-sess-1".into()),
            ],
            kept: vec![],
        }
    );
    let git = ix.git_worktree_fake();
    assert_eq!(
        *git.removed_worktrees.lock().unwrap(),
        vec![(REPO_ROOT.to_owned(), path.clone())]
    );
    assert_eq!(*git.pruned.lock().unwrap(), vec![REPO_ROOT.to_owned()]);
    assert_eq!(*git.forgotten.lock().unwrap(), vec![path]);
    assert_eq!(
        *git.deleted_branches.lock().unwrap(),
        vec![(REPO_ROOT.to_owned(), "delta-sess-1".to_owned())]
    );
}
