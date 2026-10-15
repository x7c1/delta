//! API commands: the session lifecycle a client drives — open, close,
//! delete, interrupt, clear the pane's residual input. The `enqueue_send`
//! module holds the enqueue that routes a user's text to the session its
//! target names.

use delta_model::SessionId;
use tokio::sync::oneshot;

use crate::error::Result;
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::session_removal::SessionRemoval;

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Spawn a fresh Claude Code session in `workdir` with no initial send.
    ///
    /// Test-only: the product starts every session from a composer send
    /// (`SendTarget::NewSession`), which always carries its first prompt.
    /// Tests that only need a live pane use this prompt-less spawn instead.
    #[cfg(test)]
    pub(crate) async fn new_session_in(&self, workdir: String) -> Result<()> {
        let id = Self::mint_session_id();
        self.request(&id, |reply| SessionInput::SpawnFresh {
            first_prompt: None,
            workdir,
            launch_option_ids: Vec::new(),
            worktree: None,
            provider: delta_model::AgentProvider::Claude,
            pull_request_number: None,
            reply,
        })
        .await?;
        Ok(())
    }

    /// Resume a closed but known session: a fresh tmux session for Claude, or a
    /// `thread/resume` adapter reconnect for a terminal-less Codex session.
    pub async fn open_session(&self, id: &SessionId) -> Result<()> {
        self.request(id, |reply| SessionInput::OpenSession { reply })
            .await
    }

    /// Close an open session: capture its final transcript line, kill its
    /// pane, and drop its binding. The conversational data remains in the
    /// store. Unknown ids are a clean `SessionNotFound`.
    ///
    /// Returns the events the teardown produced, in order: a
    /// [`SessionEvent::PermissionResolved`] for every request the close
    /// stranded, then any [`SessionEvent::SubagentFinished`]s the process-gone
    /// sweep produced (a lingering background subagent cleared because its
    /// completion notification can no longer arrive). The transport broadcasts
    /// them, then `SessionClosed`.
    pub async fn close_session(&self, id: &SessionId) -> Result<Vec<SessionEvent>> {
        self.request(id, |reply| SessionInput::CloseSession { reply })
            .await
    }

    /// Remove a closed session from Delta: delete its row and, by cascade,
    /// every row that hangs off it, then the worktree and branch Delta created
    /// for it when they hold no work.
    ///
    /// Returns what was removed and what was kept on disk, for the transport to
    /// log; a kept item never refuses the removal. See
    /// `SessionContext::delete_session` for the rule, and for the two states
    /// this is refused in (open, still starting). Unknown ids are a clean
    /// `SessionNotFound`, as for [`Self::close_session`].
    pub async fn delete_session(&self, id: &SessionId) -> Result<SessionRemoval> {
        self.request(id, |reply| SessionInput::DeleteSession { reply })
            .await
    }

    /// Interrupt a session's in-flight turn, keeping the session open.
    ///
    /// For a terminal-less agent (Codex) this drives the adapter's `interrupt`
    /// (sending `turn/interrupt` on the provider's wire) without tearing the
    /// session down, so the provider's `turn/completed{interrupted}` still
    /// arrives on the session's event pump and settles the turn — the resulting
    /// [`SessionEvent::TurnInterrupted`] reaches the browser over the async
    /// event seam, so there is nothing to return here.
    ///
    /// A no-op (returning `Ok`) when the session has no actor — a session with
    /// no actor is closed by definition, and a closed or pane-backed (Claude)
    /// session carries no open agent to interrupt. Claude's turn interrupt is
    /// TUI-driven (Escape in the pane) with its own transcript-marker path,
    /// which this REST route deliberately does not duplicate.
    ///
    /// [`SessionEvent::TurnInterrupted`]: crate::ports::SessionEvent::TurnInterrupted
    pub async fn interrupt(&self, id: &SessionId) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        if !self
            .sessions
            .post_existing(id, SessionInput::Interrupt { reply: tx })
        {
            return Ok(());
        }
        rx.await.unwrap_or(Ok(()))
    }

    /// Wipe the residual input of a session's pane, if it is open. A no-op
    /// (returning `Ok`) when the session is not open — including when it has
    /// no actor at all.
    pub async fn clear_session_input(&self, id: &SessionId) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        if !self
            .sessions
            .post_existing(id, SessionInput::ClearInput { reply: tx })
        {
            return Ok(());
        }
        rx.await.unwrap_or(Ok(()))
    }
}
