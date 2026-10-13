use std::time::{Duration, UNIX_EPOCH};

use crate::interactor::testing::*;
use crate::ports::WorktreeRemoval;
use crate::{DiskItem, KeepReason, KeptItem, PruneCriteria, PruneStatus, SessionKeptItem};

use super::removal_support::{closed_session, worktree_path, REPO_ROOT};

/// The settling of a shared worktree holds beyond two clean sessions: three
/// sessions on one worktree leave its merged branch deleted, asked once; and
/// a shared worktree with changes is kept once, with
/// its `delta-<id>` branch kept once because the worktree stays — never as
/// "Delta did not create it" for a session that merely shared it.
#[tokio::test]
async fn prune_sessions_settles_a_dirty_or_thrice_shared_worktree() {
    let (three, dirty) = (worktree_path("three"), worktree_path("dirty"));
    let ix = interactor_with_git(
        FakeGitWorktree::default()
            .with_worktree_removal(&dirty, Scripted::Answer(WorktreeRemoval::KeptDirty)),
    );
    for (id, cwd, branch) in [
        ("three", &three, "delta-three"),
        ("three-2", &three, "delta-three"),
        ("three-3", &three, "delta-three"),
        ("dirty", &dirty, "delta-dirty"),
        ("dirty-2", &dirty, "delta-dirty"),
    ] {
        closed_session(&ix, id, cwd, Some(REPO_ROOT), Some(branch)).await;
    }
    let criteria = PruneCriteria {
        older_than_days: 0,
        statuses: PruneStatus::ALL.to_vec(),
    };
    let now = UNIX_EPOCH + Duration::from_secs(4_102_444_800); // 2100-01-01

    let report = ix.prune_sessions(&criteria, now).await.unwrap();

    assert_eq!(report.removed.len(), 5);
    assert!(report.skipped.is_empty());
    let mut kept: Vec<_> = report
        .kept
        .iter()
        .map(|SessionKeptItem { kept, .. }| kept.clone())
        .collect();
    kept.sort_by_key(|kept| format!("{:?}", kept.item));
    assert_eq!(
        kept,
        vec![
            KeptItem {
                item: DiskItem::Branch("delta-dirty".into()),
                reason: KeepReason::WorktreeKept,
            },
            KeptItem {
                item: DiskItem::Worktree(dirty.clone()),
                reason: KeepReason::Dirty,
            },
        ],
        "the dirty worktree and its branch are listed once each"
    );
    let git = ix.git_worktree_fake();
    let deleted: Vec<_> = git
        .deleted_branches
        .lock()
        .unwrap()
        .iter()
        .map(|(_, branch)| branch.clone())
        .collect();
    assert_eq!(
        deleted,
        ["delta-three"],
        "the thrice-shared worktree's merged branch goes, asked once"
    );
    assert!(
        git.removed_worktrees
            .lock()
            .unwrap()
            .iter()
            .any(|(_, path)| *path == three),
        "the thrice-shared worktree goes"
    );
}
