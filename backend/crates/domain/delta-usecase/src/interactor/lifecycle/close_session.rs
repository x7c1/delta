use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Close an open session: capture its final transcript line, kill its pane,
    /// and drop its binding.
    ///
    /// The conversational data remains in the store; only the live pane and the
    /// `claude` process are torn down. Closing a known session that is not open
    /// is a no-op (it has no live pane to tear down), but an *unknown* id is a
    /// clean `SessionNotFound` (404) — the same rejection [`Self::open_session`]
    /// gives — so the browser can tell "already closed" apart from "no such
    /// session" rather than having a stale id silently succeed.
    ///
    /// Once closed, a session loses its live pane and the background tail
    /// no longer polls it. But Claude Code may flush the turn's final
    /// assistant line to the JSONL just *after* its `Stop` hook fired, so
    /// without one last sync that line would never be ingested. So before the
    /// pane is dropped this runs [`Self::sync_transcript`] once — while the
    /// on-disk transcript still reflects this session's own run — to capture
    /// any straggler line.
    ///
    /// Returns any [`SessionEvent::SubagentFinished`]s produced by the
    /// process-gone sweep (see [`Self::sweep_running_subagents_on_process_gone`]),
    /// for the caller to broadcast. Closing tears down the `claude` process, so
    /// a lingering background subagent's completion notification can no longer
    /// arrive to clear its indicator; the sweep clears it here instead.
    pub(in crate::interactor) async fn close_session(&mut self) -> Result<Vec<SessionEvent>> {
        let Some(session) = self.store.session(self.id).await? else {
            return Err(Error::SessionNotFound(self.id.as_str().to_owned()));
        };
        // Final sync to capture a last line flushed after `Stop`, before the
        // session loses its pane. A closed-but-known session that is being
        // re-closed has no live pane; the sync is still safe (it just finds no
        // new lines), so it runs unconditionally on the known path.
        self.sync_transcript(&session).await?;
        if let Some(handle) = self.state.remove_open() {
            self.tmux.kill_session(handle.token.as_str()).await?;
        }
        // A terminal-less agent session (Codex) has no pane to kill; close it
        // through its adapter instead, which tears down the session's local
        // plumbing (the shared `codex app-server` connection stays up for any
        // other threads). Claude sessions have no `open_agent`, so this is a
        // no-op for them and their close path is unchanged.
        if let Some(agent) = self.state.remove_open_agent() {
            agent.adapter.close(&agent.handle).await?;
        }
        // Deliberate no-op for git worktrees (MVP): a session that started in a
        // worktree keeps it on close. `session.cwd` is the worktree path, so a
        // later resume reattaches to the still-present worktree rather than
        // recreating it. Removing the worktree (and its branch) on close is
        // deferred until there is an explicit cleanup story; doing it here would
        // throw away uncommitted work the moment a session is closed.
        // The pane is gone, so whatever turn was in flight can no longer
        // progress: feed `Close` into the turn machine (an unechoed outstanding
        // send is cancelled; an in-flight one is swept if it never matched).
        self.apply_turn_input(crate::turn::TurnInput::Close).await?;
        // The `claude` process is torn down, so no more transcript is ingested:
        // a lingering BACKGROUND subagent's completion `<task-notification>` can
        // never be folded to clear its indicator. The `Close` above swept the
        // foreground entries; sweep the surviving background ones so they do not
        // stick forever, returning a `SubagentFinished` per entry to broadcast.
        self.sweep_running_subagents_on_process_gone().await
    }
}
