//! API commands: the session lifecycle a client drives — spawn, open, close,
//! delete, interrupt, clear the pane's residual input. The `enqueue_send`
//! module holds the enqueue that routes a user's text to the session its
//! target names.

use delta_model::SessionId;
use tokio::sync::oneshot;

use crate::error::Result;
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::Interactor;
use crate::pane_token::PaneToken;
use crate::ports::{
    GitWorktree, SessionEvent, SessionLifecycle, SessionStore, TmuxDriver, Transcript, Workspace,
};

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Spawn a fresh Claude Code session with no initial send (cold-start).
    pub async fn new_session(&self) -> Result<PaneToken> {
        let id = Self::mint_session_id();
        let spawn = self
            .request(&id, |reply| SessionInput::SpawnFresh {
                first_prompt: None,
                workdir: None,
                // A cold-start session (no first prompt) applies no launch
                // options; those ride only on a composer-initiated new session.
                launch_option_ids: Vec::new(),
                // A cold-start session never opts into a worktree (no workdir,
                // no first prompt); the worktree path rides only on a
                // composer-initiated new session.
                worktree: None,
                // Cold start is the Claude tmux path; a Codex session is only
                // ever created from a composer-initiated new session (which
                // carries a first prompt and its own provider selection).
                provider: delta_model::AgentProvider::Claude,
                // A cold start has no PR origin: the PR tab lives on the
                // composer-initiated new-session path alone.
                pull_request_number: None,
                reply,
            })
            .await?;
        Ok(spawn
            .token
            .expect("a Claude cold-start spawn always mints a pane token"))
    }

    /// Ensure at least one Claude Code session is up, spawning one if absent.
    ///
    /// Idempotent for the still-single server surface: if any session's actor
    /// holds a live pane it is reused and [`SessionLifecycle::Ready`] is
    /// returned with no side effects. Otherwise a fresh session is spawned and
    /// [`SessionLifecycle::Starting`] returned. "Live" spans the whole starting
    /// window — a session that is bound, one whose pane is up and awaiting its
    /// first bind, *and* one that has only been accepted, with its launch
    /// preparation still running — so a second cold start arriving while the
    /// first session's worktree is being checked out reuses it instead of
    /// starting a rival session (`SessionRuntime::has_live_pane`).
    pub async fn ensure_session(&self) -> Result<SessionLifecycle> {
        if !self.live_session_ids().await.is_empty() {
            return Ok(SessionLifecycle::Ready);
        }
        self.new_session().await?;
        Ok(SessionLifecycle::Starting)
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
    /// every row that hangs off it.
    ///
    /// Delta's own rows are all that go. Nothing on disk is touched — see
    /// `SessionContext::delete_session` for why, and for the two states this is
    /// refused in (open, still starting). Unknown ids are a clean
    /// `SessionNotFound`, as for [`Self::close_session`].
    pub async fn delete_session(&self, id: &SessionId) -> Result<()> {
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
