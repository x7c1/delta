//! Removing a closed session from Delta's list.

use delta_model::SessionStatus;

use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Delete a closed session's rows, taking it off the session list for good.
    ///
    /// **What this removes is Delta's own data, and nothing else.** The `session`
    /// row goes and the cascade takes every row that hangs off it (threads,
    /// messages, sends, permission requests, subagents, the sync cursor). What
    /// stays is everything outside the database: the git worktree the session
    /// ran in — which may hold uncommitted work — its branch, and the agent's
    /// own transcript and state files (Claude Code's JSONL, Codex's thread).
    /// "Delete" invites the opposite assumption, so it is worth being plain:
    /// this is the user tidying Delta's list, not a command to destroy work.
    /// Closing already keeps the worktree so a session can be resumed
    /// ([`Self::close_session`]); removal does not change that bargain, it just
    /// stops Delta from listing a conversation the user is finished with.
    ///
    /// Removal is offered only for a session that is **neither open nor still
    /// starting**, and the two forbidden states are refused separately because
    /// they ask the user for different things:
    ///
    /// - **Still starting** — a runtime launch that has not bound (its
    ///   preparation still running, or its pane up and awaiting its first hook),
    ///   or a row that still says `spawning`. Refused with
    ///   [`Error::SessionSpawning`]: wait for it to come up, or close it, which
    ///   cancels the launch and removes the eager row anyway.
    /// - **Open** — a live pane (Claude) or a live terminal-less agent session
    ///   (Codex). Refused with [`Error::SessionOpen`]: close it first. Open-ness
    ///   is process-runtime state, not a column (a restart rebuilds it empty),
    ///   so it is read off this session's own runtime — the same authority the
    ///   session list annotates each row with — rather than inferred from
    ///   [`SessionStatus`].
    ///
    /// Order matters: the state is checked *before* anything is deleted, so a
    /// refusal leaves every row exactly as it was. An unknown id is a clean
    /// [`Error::SessionNotFound`] (404), as [`Self::close_session`] gives, so a
    /// stale card cannot silently succeed.
    ///
    /// The caller broadcasts `SessionRemoved`; there is nothing to return.
    pub(in crate::interactor) async fn delete_session(&mut self) -> Result<()> {
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
        tracing::info!(
            session_id = %self.id,
            "removing a closed session; its worktree and the agent's own files are left in place"
        );
        self.store.delete_session(self.id).await?;
        // The row (and every send row, by cascade) is gone, so the actor's turn
        // state has nothing left to refer to: drop it without orphan handling,
        // exactly as the launch rollback does for the row it deletes. A closed
        // session is usually already idle, but one Delta never held a pane for
        // (an external agent registered by its hooks) can carry a turn and a
        // background subagent that no completion can ever finish — and a kept
        // running entry would pin this doomed actor alive for the process's
        // lifetime.
        self.state.forget_turn();
        Ok(())
    }
}
