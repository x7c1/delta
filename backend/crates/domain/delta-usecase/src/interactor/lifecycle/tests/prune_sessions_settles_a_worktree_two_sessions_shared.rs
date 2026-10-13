use std::time::{Duration, UNIX_EPOCH};

use crate::interactor::testing::*;
use crate::ports::BranchDeletion;
use crate::{DiskItem, KeepReason, KeptItem, PruneCriteria, PruneStatus, SessionKeptItem};

use super::removal_support::{closed_session, worktree_path, REPO_ROOT};

/// Two sessions can share one worktree (the second was started on the first
/// one's branch). Pruning both removes the worktree with the second, and then
/// settles the first one's `delta-<id>` branch as its own removal would have:
/// the merged branch is deleted, the unmerged one is reported once, as
/// unmerged — never as kept for a worktree that is gone.
#[tokio::test]
async fn prune_sessions_settles_a_worktree_two_sessions_shared() {
    let (merged, unmerged) = (worktree_path("merged"), worktree_path("unmerged"));
    let ix = interactor_with_git(FakeGitWorktree::default().with_branch_deletion(
        "delta-unmerged",
        Scripted::Answer(BranchDeletion::KeptUnmerged),
    ));
    for (id, cwd, branch) in [
        ("merged", &merged, "delta-merged"),
        ("merged-2", &merged, "delta-merged"),
        ("unmerged", &unmerged, "delta-unmerged"),
        ("unmerged-2", &unmerged, "delta-unmerged"),
    ] {
        closed_session(&ix, id, cwd, Some(REPO_ROOT), Some(branch)).await;
    }
    let criteria = PruneCriteria {
        older_than_days: 0,
        statuses: PruneStatus::ALL.to_vec(),
    };
    let now = UNIX_EPOCH + Duration::from_secs(4_102_444_800); // 2100-01-01

    let report = ix.prune_sessions(&criteria, now).await.unwrap();

    assert_eq!(report.removed.len(), 4);
    assert!(report.skipped.is_empty());
    assert_eq!(
        report.kept,
        vec![SessionKeptItem {
            session_id: "unmerged".into(),
            kept: KeptItem {
                item: DiskItem::Branch("delta-unmerged".into()),
                reason: KeepReason::Unmerged,
            },
        }],
        "only the unmerged branch remains, listed once"
    );
    let git = ix.git_worktree_fake();
    let mut removed_worktrees: Vec<_> = git
        .removed_worktrees
        .lock()
        .unwrap()
        .iter()
        .map(|(_, path)| path.clone())
        .collect();
    removed_worktrees.sort();
    assert_eq!(removed_worktrees, [merged, unmerged]);
    let mut deleted: Vec<_> = git
        .deleted_branches
        .lock()
        .unwrap()
        .iter()
        .map(|(_, branch)| branch.clone())
        .collect();
    deleted.sort();
    assert_eq!(
        deleted,
        ["delta-merged", "delta-unmerged"],
        "git is asked to delete each shared worktree's branch once, unforced"
    );
}
