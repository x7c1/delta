//! Erasing everything Delta created on this machine that holds no work: every
//! session, Delta's tmux server, and the worktrees under the worktree base.

use std::time::SystemTime;

use delta_model::{SessionId, SessionStatus};

use crate::erase_report::EraseReport;
use crate::error::{Error, Result};
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::session_prune::{PruneCriteria, SkipReason};
use crate::session_removal::{DiskItem, KeepReason, KeptItem};

/// Every row status, so the session listing below takes every stored session.
const EVERY_STATUS: [SessionStatus; 4] = [
    SessionStatus::Spawning,
    SessionStatus::Active,
    SessionStatus::Ended,
    SessionStatus::Failed,
];

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Remove everything Delta created that holds no work, and report what was
    /// removed and what was kept.
    ///
    /// **Delta never destroys the user's work: it removes what it created, and
    /// only when that holds no work.** There is no `force`. In order:
    ///
    /// 1. Every session that is open or still starting is closed through
    ///    [`Self::close_session`], which cancels a launch in progress and stops
    ///    a terminal-less agent's process. Nothing is refused for being open.
    /// 2. Delta's tmux server is killed and its socket file removed
    ///    ([`TmuxDriver::kill_server`]) — before any worktree is touched, so no
    ///    agent process can still hold a working directory or write into one.
    /// 3. Every session is removed through the single removal
    ///    ([`Self::delete_session`]), so the rule on the worktree and branch
    ///    Delta created for it applies unchanged: a dirty worktree and an
    ///    unmerged branch are kept, and so is a branch Delta did not create.
    /// 4. Every directory left under the worktree base is removed through
    ///    [`Self::remove_worktree_dir`] without `force`: a clean worktree git
    ///    knows goes (with its trust entry), a dirty or unregistered one is
    ///    kept. Their branches are never touched, and a worktree a removed
    ///    session already kept is not reported twice. The worktree base itself is
    ///    then removed when it is empty, and so is `base_parent` (the transport
    ///    passes `~/.delta` when the base is the default `~/.delta/worktrees`).
    ///
    /// Nothing here fails the erase for one item: a session that vanished
    /// meanwhile is skipped, and every other failure is logged at `warn` and
    /// the item counts as kept (or, for a session, stays in the store the
    /// transport deletes afterwards). Only listing the sessions or the
    /// worktree base can fail the whole call, before anything is removed in
    /// the step that needed the listing.
    ///
    /// The events closing the sessions produced are not returned: the server
    /// stops right after this, and every browser with it.
    pub async fn erase_everything(
        &self,
        now: SystemTime,
        base_parent: Option<&str>,
    ) -> Result<EraseReport> {
        self.close_every_session(now).await?;
        if let Err(err) = self.tmux.kill_server().await {
            tracing::warn!(error = %err, "killing Delta's tmux server failed; erasing the rest");
        }
        let mut report = EraseReport::default();
        self.remove_every_session(now, &mut report).await?;
        self.remove_leftover_worktrees(&mut report).await?;
        self.remove_empty_worktree_base(base_parent).await;
        tracing::info!(
            sessions = report.removed_sessions.len(),
            removed = report.removed.len(),
            kept = report.kept.len() + report.kept_leftovers.len(),
            "erased everything Delta created that holds no work"
        );
        Ok(report)
    }

    /// Step 1: close every session that is live (open, or with a launch in
    /// flight), and every row a restart left `spawning`.
    async fn close_every_session(&self, now: SystemTime) -> Result<()> {
        let mut ids = self.live_session_ids().await;
        for id in self
            .store
            .list_prunable_sessions(&every_session_until(now), &[SessionStatus::Spawning])
            .await?
        {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        for session_id in ids {
            match self.close_session(&session_id).await {
                Ok(_) => tracing::info!(%session_id, "closed session before erasing"),
                Err(Error::SessionNotFound(_)) => {}
                Err(err) => tracing::warn!(
                    %session_id,
                    error = %err,
                    "closing a session before erasing failed; removing it anyway"
                ),
            }
        }
        Ok(())
    }

    /// Step 3: remove every stored session through the single removal.
    async fn remove_every_session(&self, now: SystemTime, report: &mut EraseReport) -> Result<()> {
        let ids: Vec<SessionId> = self
            .store
            .list_prunable_sessions(&every_session_until(now), &EVERY_STATUS)
            .await?;
        let removal = self.remove_sessions(ids, &mut report.removed).await;
        for skipped in &removal.skipped {
            if skipped.reason != SkipReason::Gone {
                tracing::warn!(
                    session_id = %skipped.session_id,
                    reason = ?skipped.reason,
                    "a session could not be removed while erasing; its rows go with the database"
                );
            }
        }
        report.removed_sessions = removal.removed;
        report.kept = removal.kept;
        Ok(())
    }

    /// Step 4: remove each clean leftover under the worktree base and keep the
    /// rest, with the reason the single Storage removal refused it.
    async fn remove_leftover_worktrees(&self, report: &mut EraseReport) -> Result<()> {
        for dir in self.list_worktree_dirs().await? {
            let path = dir.path;
            // A worktree a removed session kept is already reported, with its
            // session and the reason; listing it again as a leftover would
            // only say the same thing twice.
            let kept_by_session = report.kept.iter().any(|kept| {
                matches!(&kept.kept.item, DiskItem::Worktree(kept_path) if *kept_path == path)
            });
            if kept_by_session {
                continue;
            }
            match self.remove_worktree_dir(&path, false).await {
                Ok(()) => report.removed.push(DiskItem::Worktree(path)),
                Err(err) => {
                    let reason = leftover_keep_reason(err);
                    if let KeepReason::Failed(error) = &reason {
                        tracing::warn!(
                            path,
                            error,
                            "removing a leftover worktree failed; keeping it"
                        );
                    }
                    report.kept_leftovers.push(KeptItem {
                        item: DiskItem::Worktree(path),
                        reason,
                    });
                }
            }
        }
        Ok(())
    }

    /// Remove the worktree base when it is empty, then `base_parent` when that
    /// is empty too. A directory still holding anything stays.
    async fn remove_empty_worktree_base(&self, base_parent: Option<&str>) {
        for dir in std::iter::once(self.worktree_base.as_str()).chain(base_parent) {
            match self.workspace.remove_empty_dir(dir).await {
                Ok(true) => tracing::info!(dir, "removed the emptied directory"),
                Ok(false) => {
                    tracing::info!(dir, "kept a directory that is not empty");
                    return;
                }
                Err(err) => {
                    tracing::warn!(dir, error = %err, "removing an emptied directory failed");
                    return;
                }
            }
        }
    }
}

/// The recency cut-off that takes every session stored at `now`, whatever
/// its age.
fn every_session_until(now: SystemTime) -> String {
    PruneCriteria {
        older_than_days: 0,
        statuses: Vec::new(),
    }
    .cutoff(now)
}

/// Why the Storage removal refused a leftover, as the reason it is kept.
fn leftover_keep_reason(err: Error) -> KeepReason {
    match err {
        Error::WorktreeDirty(_) => KeepReason::Dirty,
        Error::WorktreeNotRegistered(_) => KeepReason::NotRegistered,
        Error::WorktreeInUse(_) => KeepReason::InUseByAnotherSession,
        other => KeepReason::Failed(other.to_string()),
    }
}
