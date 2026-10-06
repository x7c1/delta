//! Removing a closed session from Delta's list, with the worktree and branch
//! Delta created for it when they hold no work.

use delta_model::{Session, SessionStatus};

use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{
    BranchDeletion, GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace, WorktreeRemoval,
};
use crate::session_removal::{DiskItem, KeepReason, SessionRemoval};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Delete a closed session's rows, taking it off the session list for good,
    /// and remove the worktree and branch Delta created for it when they hold
    /// no work.
    ///
    /// **A session owns the worktree and the branch Delta created for it, and
    /// Delta never destroys the user's work.** The `session` row goes and the
    /// cascade takes every row that hangs off it (threads, messages, sends,
    /// permission requests, subagents, the sync cursor). Then, if the session
    /// ran in a worktree Delta created, that worktree is removed when it is
    /// clean and its `delta-<session id>` branch deleted when it is merged —
    /// see [`Self::remove_owned_worktree`] for the exact rule. Whatever holds
    /// work is kept and reported in the returned [`SessionRemoval`], for the
    /// transport to log. The agent's own
    /// transcript and state files (Claude Code's JSONL, Codex's thread) are
    /// never touched.
    ///
    /// Removal is offered only for a session that is **neither open nor still
    /// starting**, and the two forbidden states are refused separately because
    /// they ask the user for different things:
    ///
    /// - **Still starting** — a runtime launch that has not bound (its
    ///   preparation still running, or its pane up and awaiting its first hook),
    ///   or a row that still says `spawning`. Refused with
    ///   [`Error::SessionSpawning`]: wait for it to come up, or close it, which
    ///   cancels the launch and leaves the row `failed`, which can be removed.
    /// - **Open** — a live pane (Claude) or a live terminal-less agent session
    ///   (Codex). Refused with [`Error::SessionOpen`]: close it first. Open-ness
    ///   is process-runtime state, not a column (a restart rebuilds it empty),
    ///   so it is read off this session's own runtime — the same authority the
    ///   session list annotates each row with — rather than inferred from
    ///   [`SessionStatus`].
    ///
    /// Order matters. The state is checked *before* anything is deleted, so a
    /// refusal leaves every row and every file exactly as it was. The row is
    /// deleted *before* the disk is touched: the row must go even when git
    /// fails, so a git failure can only ever mean "kept", never a refused
    /// removal; the worktree path has already been read off the row; and a row
    /// deletion that fails leaves the worktree alone rather than removing the
    /// working copy of a session Delta still lists. An unknown id is a clean
    /// [`Error::SessionNotFound`] (404), as [`Self::close_session`] gives, so a
    /// stale card cannot silently succeed.
    ///
    /// The caller broadcasts `SessionRemoved`.
    pub(in crate::interactor) async fn delete_session(&mut self) -> Result<SessionRemoval> {
        let Some(session) = self.store.session(self.id).await? else {
            return Err(Error::SessionNotFound(self.id.as_str().to_owned()));
        };
        // Checked first, and without consuming anything: a refused removal must
        // leave the launch to bind (or to be cancelled by a close) normally.
        if self.state.is_launching_or_pending() || session.status == SessionStatus::Spawning {
            return Err(Error::SessionSpawning(self.id.as_str().to_owned()));
        }
        if self.state.is_open() {
            return Err(Error::SessionOpen(self.id.as_str().to_owned()));
        }
        self.store.delete_session(self.id).await?;
        // The row (and every send row, by cascade) is gone, so the actor's turn
        // state has nothing left to refer to: drop it without orphan handling,
        // exactly as a launch that ended drops the turn nothing will ever
        // drain. A closed session is usually already idle, but one Delta never
        // held a pane for (an external agent registered by its hooks) can carry
        // a turn and a background subagent that no completion can ever finish —
        // and a kept running entry would pin this doomed actor alive for the
        // process's lifetime.
        self.state.forget_turn();
        Ok(self.remove_owned_worktree(&session).await)
    }

    /// Remove the worktree and branch Delta created for `session` (whose row is
    /// already deleted) when they hold no work, and report what was removed
    /// and what was kept.
    ///
    /// - The session's `cwd` is a worktree Delta created **iff** it lies under
    ///   `worktree_base` and `repo_root` (the repository the worktree was cut
    ///   from) is set and lies outside it. Anything else — the user's own
    ///   directory, a scratch directory, an existing worktree the user picked
    ///   as a plain working directory — is not Delta's, and git is not asked
    ///   anything.
    /// - A worktree another listed session still works in is kept: removing
    ///   it would pull the working copy out from under that session.
    /// - The worktree is removed only when it is clean (`git worktree remove`
    ///   without `--force`). After a removal, `git worktree prune` runs and
    ///   the `~/.claude.json` trust entry Delta seeded for the path is
    ///   forgotten.
    /// - The branch is deleted only when it is the `delta-<session id>` branch
    ///   Delta cut for the session, only after its worktree is gone (git
    ///   refuses a branch still checked out), and only when merged
    ///   (`git branch -d`). A session started on an existing branch never has
    ///   it deleted.
    ///
    /// A git failure other than git's own refusal is logged at `warn` and the
    /// item counts as kept; nothing here fails the removal.
    async fn remove_owned_worktree(&self, session: &Session) -> SessionRemoval {
        let mut removal = SessionRemoval::default();
        let path = session.cwd.as_str();
        let Some(repo_root) = session.repo_root.as_deref() else {
            return removal;
        };
        if !super::is_under_worktree_base(&self.worktree_base, path)
            || super::is_at_or_below(repo_root, path)
        {
            return removal;
        }
        let worktree = DiskItem::Worktree(path.to_owned());
        let delta_branch = format!("delta-{}", self.id.as_str());
        // The branch the session worked on: Delta's own `delta-<id>` branch, or
        // an existing branch the user started it on (never Delta's to delete).
        let branch = session.branch_at_launch.as_deref().map(|name| {
            if name == delta_branch {
                (DiskItem::Branch(delta_branch.clone()), None)
            } else {
                (
                    DiskItem::Branch(name.to_owned()),
                    Some(KeepReason::NotCreatedByDelta),
                )
            }
        });
        let keep_branch = |removal: &mut SessionRemoval, reason: KeepReason| {
            if let Some((item, own_reason)) = &branch {
                removal.kept(item.clone(), own_reason.clone().unwrap_or(reason));
            }
        };

        match self.store.cwd_exists(path).await {
            Ok(false) => {}
            Ok(true) => {
                removal.kept(worktree, KeepReason::InUseByAnotherSession);
                keep_branch(&mut removal, KeepReason::WorktreeKept);
                return removal;
            }
            Err(err) => {
                tracing::warn!(
                    session_id = %self.id,
                    path,
                    error = %err,
                    "could not check whether another session uses the worktree; keeping it"
                );
                removal.kept(worktree, KeepReason::Failed(err.to_string()));
                keep_branch(&mut removal, KeepReason::WorktreeKept);
                return removal;
            }
        }

        match self.git_worktree.remove_worktree(repo_root, path).await {
            Ok(WorktreeRemoval::Removed) => removal.removed(worktree),
            Ok(WorktreeRemoval::KeptDirty) => {
                removal.kept(worktree, KeepReason::Dirty);
                keep_branch(&mut removal, KeepReason::WorktreeKept);
                return removal;
            }
            Err(err) => {
                tracing::warn!(
                    session_id = %self.id,
                    repo_root,
                    path,
                    error = %err,
                    "removing the session's worktree failed; keeping it"
                );
                removal.kept(worktree, KeepReason::Failed(err.to_string()));
                keep_branch(&mut removal, KeepReason::WorktreeKept);
                return removal;
            }
        }

        // Housekeeping after a removal: nothing is kept or lost if it fails
        // (the worktree is already gone, and a later prune catches the entry).
        if let Err(err) = self.git_worktree.prune_worktrees(repo_root).await {
            tracing::warn!(
                session_id = %self.id,
                repo_root,
                error = %err,
                "git worktree prune failed after removing the session's worktree"
            );
        }
        let trust = DiskItem::TrustEntry(path.to_owned());
        match self.git_worktree.forget_dir_trusted(path).await {
            Ok(()) => removal.removed(trust),
            Err(err) => {
                tracing::warn!(
                    session_id = %self.id,
                    path,
                    error = %err,
                    "forgetting the removed worktree's trust entry failed"
                );
                removal.kept(trust, KeepReason::Failed(err.to_string()));
            }
        }

        match branch {
            None => {}
            Some((item, Some(reason))) => removal.kept(item, reason),
            Some((item, None)) => {
                match self
                    .git_worktree
                    .delete_branch_if_merged(repo_root, &delta_branch)
                    .await
                {
                    Ok(BranchDeletion::Deleted) => removal.removed(item),
                    Ok(BranchDeletion::KeptUnmerged) => removal.kept(item, KeepReason::Unmerged),
                    Ok(BranchDeletion::Absent) => {}
                    Err(err) => {
                        tracing::warn!(
                            session_id = %self.id,
                            repo_root,
                            branch = %delta_branch,
                            error = %err,
                            "deleting the session's branch failed; keeping it"
                        );
                        removal.kept(item, KeepReason::Failed(err.to_string()));
                    }
                }
            }
        }
        removal
    }
}
