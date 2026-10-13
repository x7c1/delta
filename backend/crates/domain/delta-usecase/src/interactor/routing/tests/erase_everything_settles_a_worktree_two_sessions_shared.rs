use crate::interactor::testing::*;
use crate::ports::BranchDeletion;
use crate::{DiskItem, KeepReason, KeptItem, SessionKeptItem};

use super::support::{
    closed_session_in_worktree_of, closed_worktree_session, now, worktree_of, BASE_PARENT,
};

/// Two sessions can share one worktree. Erasing removes them one at a time:
/// the first one's removal keeps the worktree for the second, and the
/// second's removes it. The first one's `delta-<id>` branch is then settled
/// too, so the merged one goes and the unmerged one is reported once, against
/// the session Delta cut it for — not as kept because its worktree was.
#[tokio::test]
async fn erase_everything_settles_a_worktree_two_sessions_shared() {
    let ix = interactor_with_tmux_workspace_and_git(
        FakeTmux::default(),
        FakeWorkspace::default(),
        FakeGitWorktree::default().with_branch_deletion(
            "delta-unmerged",
            Scripted::Answer(BranchDeletion::KeptUnmerged),
        ),
    );
    closed_worktree_session(&ix, "merged").await;
    closed_session_in_worktree_of(&ix, "merged-2", "merged").await;
    let unmerged = closed_worktree_session(&ix, "unmerged").await;
    closed_session_in_worktree_of(&ix, "unmerged-2", "unmerged").await;

    let report = ix.erase_everything(now(), Some(BASE_PARENT)).await.unwrap();

    assert_eq!(report.removed_sessions.len(), 4);
    let mut worktrees: Vec<_> = report.removed_worktrees().collect();
    worktrees.sort();
    assert_eq!(worktrees, [worktree_of("merged"), worktree_of("unmerged")]);
    assert_eq!(
        report.removed_branches().collect::<Vec<_>>(),
        ["delta-merged"],
        "the merged branch the first session left behind is deleted"
    );
    assert_eq!(
        report.kept,
        vec![SessionKeptItem {
            session_id: unmerged,
            kept: KeptItem {
                item: DiskItem::Branch("delta-unmerged".into()),
                reason: KeepReason::Unmerged,
            },
        }],
        "only what remains is listed, once"
    );
    assert!(report.kept_leftovers.is_empty());
    let mut asked: Vec<_> = ix
        .git_worktree_fake()
        .removed_worktrees
        .lock()
        .unwrap()
        .iter()
        .map(|(_, path)| path.clone())
        .collect();
    asked.sort();
    assert_eq!(
        asked,
        [worktree_of("merged"), worktree_of("unmerged")],
        "each shared worktree is removed once"
    );
}
