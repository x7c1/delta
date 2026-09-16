//! The one cleanup every never-bound launch ends in, whatever ended it.

use crate::interactor::session_actor::actor::SessionContext;
use crate::pane_token::PaneToken;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

/// What ended a never-bound launch, as the browser needs to hear it: a launch
/// that broke, or one the user asked to stop.
///
/// The reason text rides inside the variant it belongs to, so a producer cannot
/// report a breakage as a cancel — nor a cancel with no text to show for it.
pub(in crate::interactor) enum UnboundLaunchEnd {
    /// The launch broke on its own. `Some` carries the text that said why —
    /// the launch preparation's own error, or the deadline a reaped spawn
    /// missed together with what its pane was showing; `None` is for the
    /// producers with nothing at all to report (a launch that exited, a resume
    /// that never became ready).
    Failed(Option<String>),
    /// The user asked for it: an explicit close of a still-starting session.
    /// The text names the close rather than a breakage.
    Cancelled(String),
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// End an accepted-but-never-bound launch: reclaim whatever it stood up,
    /// drop the turn, mark the session row `failed` with the reason, and build
    /// the [`SessionEvent::SpawnFailed`] that reports it.
    ///
    /// Four callers reach the same end state, so they share one body:
    ///
    /// - the launch preparation failing, or reporting success it could not have
    ///   had ([`Self::finish_launch`]),
    /// - the `SessionEnd` hook of a launch that exited while still unbound
    ///   ([`Self::on_session_end`]),
    /// - the watchdog reaping a spawn that never bound before its deadline
    ///   ([`Self::reap_stale_launch`]),
    /// - and the user closing a session that is still starting
    ///   ([`Self::close_session`]), which cancels the launch outright.
    ///
    /// The caller removes whichever launch record the session was holding first
    /// (a launching entry, a pending spawn) — that record is what says *how far*
    /// the launch got, and taking it is what makes this idempotent — then calls
    /// this with:
    ///
    /// - `pane_token`: `Some` for a pane-backed (Claude) launch. It names the
    ///   tmux session to reclaim — a launch that never reached `create_session`
    ///   has no pane, which the probe-then-kill helper covers — and travels on
    ///   the event so the browser can show it. `None` for an adapter-backed
    ///   launch, which has no pane at all: passing a name tmux was never given
    ///   would answer "no such session" anyway, but asking at all would be a lie
    ///   about what this session is.
    /// - `end`: which of the two things happened, and the text the browser
    ///   shows for it ([`UnboundLaunchEnd`]).
    ///
    /// A failed row update is logged rather than propagated: the browser is
    /// waiting on a session that will never come up, and a row left reading
    /// `spawning` is the lesser problem — losing the failure report over a
    /// failed query would be the worse one.
    ///
    /// The event is *returned*, not emitted, so each caller delivers it the way
    /// its own seam does: the launch reports post it on the async event sink,
    /// while the watchdog, the hook and the close hand it back to the transport
    /// to broadcast.
    pub(in crate::interactor) async fn cancel_unbound_launch(
        &mut self,
        pane_token: Option<&PaneToken>,
        end: UnboundLaunchEnd,
    ) -> SessionEvent {
        if let Some(token) = pane_token {
            self.kill_pane_best_effort(token.as_str()).await;
        }
        // An adapter-backed launch that got as far as binding holds a live
        // provider connection (Codex: a `codex app-server` process). Close it
        // explicitly rather than relying on the drop that follows, so the
        // provider is told the thread is over and the process is reclaimed at a
        // point we can log. A no-op for a pane-backed or never-bound launch.
        if let Some(agent) = self.state.remove_open_agent() {
            if let Err(close_err) = agent.adapter.close(&agent.handle).await {
                tracing::warn!(
                    session_id = %self.id,
                    error = %close_err,
                    "failed to close the adapter of a launch that could not be \
                     completed (the connection is dropped regardless)"
                );
            }
        }
        // Nothing will ever drain this turn — the launch it belonged to is over
        // — and the session is not being deleted, so the entry is dropped
        // rather than left to pin a doomed actor's runtime state alive.
        self.state.forget_turn();
        let session_id = self.id.clone();
        let (reason, cancelled) = match end {
            UnboundLaunchEnd::Failed(reason) => (reason, false),
            UnboundLaunchEnd::Cancelled(reason) => (Some(reason), true),
        };
        // The eager row (INSERTed `spawning` when the id was minted, before
        // `claude` launched) is marked, never deleted — see
        // [`delta_model::SessionStatus::Failed`] for why the row is what the
        // failure lives on. `mark_session_failed` only touches a row still
        // reading `spawning`, so a stale reap cannot flip a session that came
        // up after all.
        if let Err(mark_err) = self
            .store
            .mark_session_failed(&session_id, reason.as_deref())
            .await
        {
            tracing::error!(
                session_id = %session_id,
                error = %mark_err,
                "failed to mark the session row of a launch that never bound as failed \
                 (it is left reading `spawning`)"
            );
        }
        SessionEvent::SpawnFailed {
            session_id,
            pane_token: pane_token.map(|token| token.as_str().to_owned()),
            reason,
            cancelled,
        }
    }
}
