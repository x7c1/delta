//! Removing a closed session from Delta's list, with the worktree and branch
//! Delta created for it when they hold no work.

use delta_model::{Session, SessionId, SessionStatus};

use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::InteractorCore;
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
    /// see [`InteractorCore::remove_owned_worktree`] for the exact rule. Whatever holds
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
        Ok(self.core.remove_owned_worktree(self.id, &session).await)
    }
}

/// The worktree Delta created for a session and the branch it worked on, read
/// off the session's row.
struct OwnedWorktree<'a> {
    path: &'a str,
    repo_root: &'a str,
    /// The `delta-<session id>` branch Delta would cut for the session.
    delta_branch: String,
    /// The branch the session worked on: Delta's own `delta-<id>` branch
    /// (no reason), or an existing branch the user started it on (never
    /// Delta's to delete, with the reason it is kept).
    branch: Option<(DiskItem, Option<KeepReason>)>,
}

impl OwnedWorktree<'_> {
    /// Keep the session's branch, for `reason` unless it is not Delta's.
    fn keep_branch(&self, removal: &mut SessionRemoval, reason: KeepReason) {
        if let Some((item, own_reason)) = &self.branch {
            removal.kept(item.clone(), own_reason.clone().unwrap_or(reason));
        }
    }
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Remove the worktree and branch Delta created for session `id` (whose
    /// row, `session`, is already deleted) when they hold no work, and report
    /// what was removed and what was kept.
    ///
    /// - The session's `cwd` is a worktree Delta created **iff** it lies under
    ///   `worktree_base` and `repo_root` (the repository the worktree was cut
    ///   from) is set and lies outside it. Anything else — the user's own
    ///   directory, a per-spawn directory an older version created, an
    ///   existing worktree the user picked as a plain working directory — is
    ///   not Delta's, and git is not asked anything.
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
    ///
    /// Also what a bulk removal runs again for a session whose worktree was
    /// kept only because a later session of the same batch still worked in it
    /// (see `Interactor::remove_sessions`).
    pub(in crate::interactor) async fn remove_owned_worktree(
        &self,
        id: &SessionId,
        session: &Session,
    ) -> SessionRemoval {
        let mut removal = SessionRemoval::default();
        let Some(owned) = self.owned_worktree(id, session) else {
            return removal;
        };
        let (path, repo_root) = (owned.path, owned.repo_root);
        let worktree = DiskItem::Worktree(path.to_owned());

        match self.store.cwd_exists(path).await {
            Ok(false) => {}
            Ok(true) => {
                removal.kept(worktree, KeepReason::InUseByAnotherSession);
                owned.keep_branch(&mut removal, KeepReason::WorktreeKept);
                return removal;
            }
            Err(err) => {
                tracing::warn!(
                    session_id = %id,
                    path,
                    error = %err,
                    "could not check whether another session uses the worktree; keeping it"
                );
                removal.kept(worktree, KeepReason::Failed(err.to_string()));
                owned.keep_branch(&mut removal, KeepReason::WorktreeKept);
                return removal;
            }
        }

        match self.git_worktree.remove_worktree(repo_root, path).await {
            Ok(WorktreeRemoval::Removed) => removal.removed(worktree),
            Ok(WorktreeRemoval::KeptDirty) => {
                removal.kept(worktree, KeepReason::Dirty);
                owned.keep_branch(&mut removal, KeepReason::WorktreeKept);
                return removal;
            }
            Err(err) => {
                tracing::warn!(
                    session_id = %id,
                    repo_root,
                    path,
                    error = %err,
                    "removing the session's worktree failed; keeping it"
                );
                removal.kept(worktree, KeepReason::Failed(err.to_string()));
                owned.keep_branch(&mut removal, KeepReason::WorktreeKept);
                return removal;
            }
        }

        // Housekeeping after a removal: nothing is kept or lost if it fails
        // (the worktree is already gone, and a later prune catches the entry).
        if let Err(err) = self.git_worktree.prune_worktrees(repo_root).await {
            tracing::warn!(
                session_id = %id,
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
                    session_id = %id,
                    path,
                    error = %err,
                    "forgetting the removed worktree's trust entry failed"
                );
                removal.kept(trust, KeepReason::Failed(err.to_string()));
            }
        }

        self.remove_owned_branch(id, &owned, &mut removal).await;
        removal
    }

    /// The branch step of [`Self::remove_owned_worktree`] alone, for session
    /// `id` (whose row, `session`, is already deleted) whose worktree another
    /// session's removal has already removed, prune and trust entry included.
    pub(in crate::interactor) async fn remove_owned_branch_of_removed_worktree(
        &self,
        id: &SessionId,
        session: &Session,
    ) -> SessionRemoval {
        let mut removal = SessionRemoval::default();
        if let Some(owned) = self.owned_worktree(id, session) {
            self.remove_owned_branch(id, &owned, &mut removal).await;
        }
        removal
    }

    /// The worktree Delta created for session `id` per its row `session`, or
    /// `None` when the session did not run in one (see
    /// [`Self::remove_owned_worktree`]).
    fn owned_worktree<'a>(
        &self,
        id: &SessionId,
        session: &'a Session,
    ) -> Option<OwnedWorktree<'a>> {
        let path = session.cwd.as_str();
        let repo_root = session.repo_root.as_deref()?;
        if !super::is_under_worktree_base(&self.worktree_base, path)
            || super::is_at_or_below(repo_root, path)
        {
            return None;
        }
        let delta_branch = format!("delta-{}", id.as_str());
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
        Some(OwnedWorktree {
            path,
            repo_root,
            delta_branch,
            branch,
        })
    }

    /// Delete the session's `delta-<id>` branch when it is merged, its
    /// worktree being gone; keep any other branch with its reason.
    async fn remove_owned_branch(
        &self,
        id: &SessionId,
        owned: &OwnedWorktree<'_>,
        removal: &mut SessionRemoval,
    ) {
        match &owned.branch {
            None => {}
            Some((item, Some(reason))) => removal.kept(item.clone(), reason.clone()),
            Some((item, None)) => {
                match self
                    .git_worktree
                    .delete_branch_if_merged(owned.repo_root, &owned.delta_branch)
                    .await
                {
                    Ok(BranchDeletion::Deleted) => removal.removed(item.clone()),
                    Ok(BranchDeletion::KeptUnmerged) => {
                        removal.kept(item.clone(), KeepReason::Unmerged)
                    }
                    Ok(BranchDeletion::Absent) => {}
                    Err(err) => {
                        tracing::warn!(
                            session_id = %id,
                            repo_root = owned.repo_root,
                            branch = %owned.delta_branch,
                            error = %err,
                            "deleting the session's branch failed; keeping it"
                        );
                        removal.kept(item.clone(), KeepReason::Failed(err.to_string()));
                    }
                }
            }
        }
    }
}
