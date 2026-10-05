use crate::interactor::testing::*;
use crate::{DiskItem, KeepReason};

use super::removal_support::{closed_session, worktree_path, REPO_ROOT};

/// A git failure that is not git's refusal — the repository is gone, `git` is
/// missing — cannot tell Delta the worktree is clean, so everything is kept,
/// the failure is logged, and the session's row is still deleted.
#[tokio::test]
async fn delete_session_keeps_everything_when_git_fails() {
    let path = worktree_path("sess-1");
    let ix = interactor_with_git(
        FakeGitWorktree::default().with_worktree_removal(&path, Scripted::GitFailure),
    );
    let id = closed_session(&ix, "sess-1", &path, Some(REPO_ROOT), Some("delta-sess-1")).await;
    let (log, _guard) = capture_warnings();

    let removal = ix
        .delete_session(&id)
        .await
        .expect("a git failure does not fail the removal");

    assert!(
        ix.store().session(&id).await.unwrap().is_none(),
        "the row is still deleted"
    );
    assert!(
        removal.removed.is_empty(),
        "nothing was removed, got {removal}"
    );
    let kept: Vec<_> = removal.kept.iter().map(|k| &k.item).collect();
    assert_eq!(
        kept,
        vec![
            &DiskItem::Worktree(path),
            &DiskItem::Branch("delta-sess-1".into())
        ]
    );
    assert!(matches!(removal.kept[0].reason, KeepReason::Failed(_)));
    let git = ix.git_worktree_fake();
    assert!(git.deleted_branches.lock().unwrap().is_empty());
    assert!(git.pruned.lock().unwrap().is_empty());
    assert!(git.forgotten.lock().unwrap().is_empty());
    assert!(
        log.text()
            .contains("removing the session's worktree failed"),
        "the failure is logged, got: {}",
        log.text()
    );
}
