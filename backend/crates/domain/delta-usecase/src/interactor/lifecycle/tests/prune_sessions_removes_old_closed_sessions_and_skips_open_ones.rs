use std::time::{Duration, SystemTime, UNIX_EPOCH};

use delta_model::{SessionId, SessionStatus};

use crate::interactor::testing::*;
use crate::ports::WorktreeRemoval;
use crate::{
    DiskItem, KeepReason, KeptItem, PruneCriteria, PruneStatus, SessionKeptItem, SkipReason,
    SkippedSession,
};

use super::removal_support::{closed_session, worktree_path, REPO_ROOT};

/// A fixed "now" far past every fixture's timestamp, so the 30-day cut-off
/// lands after all of them and only the one session dated later is too new.
fn far_future() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(4_102_444_800) // 2100-01-01
}

/// The bulk removal is the single removal applied to each old closed session:
/// a clean worktree goes, a dirty one is kept and reported against its
/// session, a failed launch goes too — and an open session that matches the
/// row criteria is skipped rather than failing the batch. The preview counts
/// exactly the sessions that are then removed.
#[tokio::test]
async fn prune_sessions_removes_old_closed_sessions_and_skips_open_ones() {
    let (clean, dirty) = (worktree_path("old-clean"), worktree_path("old-dirty"));
    let ix = interactor_with_git(
        FakeGitWorktree::default()
            .with_worktree_removal(&dirty, Scripted::Answer(WorktreeRemoval::KeptDirty)),
    );
    // Open and idle: its row is `active` like a closed session's.
    ix.seed_session().await;
    let open = SessionId::from("sess-1");
    let clean_id = closed_session(
        &ix,
        "old-clean",
        &clean,
        Some(REPO_ROOT),
        Some("delta-old-clean"),
    )
    .await;
    let dirty_id = closed_session(
        &ix,
        "old-dirty",
        &dirty,
        Some(REPO_ROOT),
        Some("delta-old-dirty"),
    )
    .await;
    let failed_id = closed_session(&ix, "old-failed", "/scratch/f", None, None).await;
    ix.store()
        .set_session_status(&failed_id, SessionStatus::Failed);
    let recent_id = closed_session(&ix, "recent", "/scratch/r", None, None).await;
    ix.store()
        .set_session_created_at(&recent_id, "2999-01-01T00:00:00Z");
    let criteria = PruneCriteria {
        older_than_days: 30,
        statuses: PruneStatus::ALL.to_vec(),
    };

    let mut candidates = ix.prune_candidates(&criteria, far_future()).await.unwrap();
    let report = ix.prune_sessions(&criteria, far_future()).await.unwrap();

    let mut removed = report.removed.clone();
    removed.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    candidates.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    assert_eq!(
        removed,
        vec![clean_id.clone(), dirty_id.clone(), failed_id.clone()]
    );
    assert_eq!(
        candidates, removed,
        "the preview names exactly what is removed"
    );
    assert_eq!(
        report.skipped,
        vec![SkippedSession {
            session_id: open.clone(),
            reason: SkipReason::Open,
        }]
    );
    assert_eq!(
        report.kept,
        vec![
            SessionKeptItem {
                session_id: dirty_id.clone(),
                kept: KeptItem {
                    item: DiskItem::Worktree(dirty),
                    reason: KeepReason::Dirty,
                },
            },
            SessionKeptItem {
                session_id: dirty_id,
                kept: KeptItem {
                    item: DiskItem::Branch("delta-old-dirty".into()),
                    reason: KeepReason::WorktreeKept,
                },
            },
        ]
    );
    for id in &report.removed {
        assert!(
            ix.store().session(id).await.unwrap().is_none(),
            "{id} is gone"
        );
    }
    assert!(
        ix.store().session(&open).await.unwrap().is_some(),
        "the open session stays"
    );
    assert!(
        ix.store().session(&recent_id).await.unwrap().is_some(),
        "the recent one stays"
    );
    assert_eq!(
        *ix.git_worktree_fake().removed_worktrees.lock().unwrap(),
        vec![
            (REPO_ROOT.to_owned(), clean),
            (REPO_ROOT.to_owned(), worktree_path("old-dirty")),
        ],
        "each worktree goes through the unforced single-session removal"
    );
}
