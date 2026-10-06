use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{BranchDeletion, WorktreeRemoval};
use crate::{DiskItem, KeepReason, KeptItem, SessionKeptItem};

use super::support::{
    closed_worktree_session, now, under_base, worktree_of, BASE_PARENT, REPO_ROOT,
};

/// Erasing closes the open session (it is not skipped), kills the tmux server
/// before any worktree is touched, then removes every session through the
/// single removal and the leftovers through the Storage removal: the clean
/// worktrees and the merged branch go, the dirty worktree, the unmerged branch
/// and the directory git does not know stay with their reasons. Last, the base
/// and then its parent are handed to `remove_empty_dir`, which the fake
/// workspace treats as empty (a base that is not is the other test).
#[tokio::test]
async fn erase_everything_closes_then_kills_the_server_then_removes_and_keeps_work() {
    let journal = CallJournal::default();
    let (leftover, stray) = (under_base("leftover"), under_base("stray"));
    let ix = interactor_with_tmux_workspace_and_git(
        FakeTmux::default().with_journal(&journal),
        FakeWorkspace::default().with_children(
            TEST_WORKTREE_BASE,
            &["leftover", "stray", "x7c1-delta-dirty"],
        ),
        FakeGitWorktree::default()
            .with_journal(&journal)
            .with_worktree_removal(
                &worktree_of("dirty"),
                Scripted::Answer(WorktreeRemoval::KeptDirty),
            )
            .with_branch_deletion(
                "delta-unmerged",
                Scripted::Answer(BranchDeletion::KeptUnmerged),
            )
            .with_inspection(&leftover, REPO_ROOT, false)
            .with_inspection(&worktree_of("dirty"), REPO_ROOT, true),
    );
    ix.seed_session().await;
    let open = SessionId::from("sess-1");
    assert!(ix.is_session_open(&open).await, "starts open");
    let clean = closed_worktree_session(&ix, "clean").await;
    let dirty = closed_worktree_session(&ix, "dirty").await;
    let unmerged = closed_worktree_session(&ix, "unmerged").await;

    let report = ix.erase_everything(now(), Some(BASE_PARENT)).await.unwrap();

    let entries = journal.entries();
    assert_eq!(
        entries[..2],
        [
            "kill_session delta-seed".to_owned(),
            "kill_server".to_owned()
        ],
        "the open session is closed, then the server killed, before any worktree: {entries:?}"
    );
    let mut session_worktrees = entries[2..5].to_vec();
    session_worktrees.sort();
    assert_eq!(
        session_worktrees,
        ["clean", "dirty", "unmerged"].map(|id| format!("remove_worktree {}", worktree_of(id))),
        "each session's worktree goes through the single removal"
    );
    assert_eq!(
        entries[5..],
        [format!("remove_worktree {leftover}")],
        "the leftovers come last"
    );
    assert_eq!(*ix.tmux_fake().servers_killed.lock().unwrap(), 1);

    let mut removed_sessions = report.removed_sessions.clone();
    removed_sessions.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    assert_eq!(
        removed_sessions,
        vec![clean, dirty.clone(), open, unmerged.clone()]
    );
    for id in &report.removed_sessions {
        assert!(
            ix.store().session(id).await.unwrap().is_none(),
            "{id} is gone"
        );
    }

    for item in [
        DiskItem::Worktree(worktree_of("clean")),
        DiskItem::Branch("delta-clean".into()),
        DiskItem::Worktree(worktree_of("unmerged")),
        DiskItem::Worktree(leftover.clone()),
    ] {
        assert!(
            report.removed.contains(&item),
            "{item} was removed: {report:?}"
        );
    }
    assert_eq!(
        report.removed_branches().collect::<Vec<_>>(),
        vec!["delta-clean"]
    );

    let mut kept = report.kept.clone();
    kept.sort_by(|a, b| a.session_id.as_str().cmp(b.session_id.as_str()));
    assert_eq!(
        kept,
        vec![
            SessionKeptItem {
                session_id: dirty.clone(),
                kept: KeptItem {
                    item: DiskItem::Worktree(worktree_of("dirty")),
                    reason: KeepReason::Dirty,
                },
            },
            SessionKeptItem {
                session_id: dirty,
                kept: KeptItem {
                    item: DiskItem::Branch("delta-dirty".into()),
                    reason: KeepReason::WorktreeKept,
                },
            },
            SessionKeptItem {
                session_id: unmerged,
                kept: KeptItem {
                    item: DiskItem::Branch("delta-unmerged".into()),
                    reason: KeepReason::Unmerged,
                },
            },
        ]
    );
    assert_eq!(
        report.kept_leftovers,
        vec![KeptItem {
            item: DiskItem::Worktree(stray),
            reason: KeepReason::NotRegistered,
        }],
        "the session's dirty worktree is reported once, with its session"
    );

    let git = ix.git_worktree_fake();
    assert!(
        git.force_removed.lock().unwrap().is_empty(),
        "nothing is forced"
    );
    assert!(
        ix.workspace_fake().removed_trees.lock().unwrap().is_empty(),
        "no directory is deleted with its contents"
    );
    assert_eq!(
        *ix.workspace_fake().removed_empty_dirs.lock().unwrap(),
        vec![TEST_WORKTREE_BASE.to_owned(), BASE_PARENT.to_owned()]
    );
}
