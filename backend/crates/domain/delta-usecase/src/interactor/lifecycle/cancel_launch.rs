//! The one cleanup every never-bound launch ends in, whatever ended it.

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::InteractorCore;
use crate::pane_token::PaneToken;
use crate::ports::{
    GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, UnsentSend, Workspace,
};

/// What ended a never-bound launch, as the browser needs to hear it: a launch
/// that broke, or one the user asked to stop.
///
/// The reason text rides inside the variant it belongs to, so a producer cannot
/// report a breakage as a cancel — nor a cancel with no text to show for it.
pub(in crate::interactor) enum UnboundLaunchEnd {
    /// The launch broke on its own. `Some` carries the text that said why (the
    /// launch preparation's own error); `None` is for the watchdog-shaped
    /// producers, which observe only silence.
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
    /// Undo an accepted-but-never-bound launch: reclaim whatever it stood up,
    /// drop the turn, delete the eager session row, and build the
    /// [`SessionEvent::SpawnFailed`] that reports it.
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
    /// A failed row cleanup is logged rather than propagated: the browser is
    /// waiting on a session that will never come up, and a row that outlived
    /// its cleanup is the lesser problem — losing the failure report over a
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
        // The session row (and every send row, by cascade) is about to be
        // deleted, so the turn entry is dropped without orphan handling.
        self.state.forget_turn();
        let session_id = self.id.clone();
        // BEFORE the cleanup, which deletes the rows this reads.
        let unsent = self.undelivered_sends(&session_id).await;
        if let Err(cleanup_err) = self.clean_up_failed_spawn_row(&session_id).await {
            tracing::error!(
                session_id = %session_id,
                error = %cleanup_err,
                "failed to clean up the eager session row of a launch that never bound"
            );
        }
        let (reason, cancelled) = match end {
            UnboundLaunchEnd::Failed(reason) => (reason, false),
            UnboundLaunchEnd::Cancelled(reason) => (Some(reason), true),
        };
        SessionEvent::SpawnFailed {
            session_id,
            pane_token: pane_token.map(|token| token.as_str().to_owned()),
            reason,
            cancelled,
            unsent,
        }
    }
}

/// The two store-facing halves of the cleanup above, in the order
/// [`SessionContext::cancel_unbound_launch`] runs them. That method is their
/// only caller, so they live beside it and reach no further than this module's
/// own `lifecycle` parent.
///
/// They sit on [`InteractorCore`] rather than on the actor's `SessionContext`
/// because neither touches runtime state: both are store calls keyed by an
/// explicitly passed session id, reached from the context above through its
/// `Deref`.
impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// The sends a failed launch accepted but never delivered to an agent,
    /// oldest first — the text the browser puts back in its composer.
    ///
    /// A spawn that never bound reached no agent at all, so *every* open send
    /// of the session qualifies: the first prompt (`dispatched` for a Claude
    /// spawn, whose prompt rides the launch command line; `queued` for an
    /// adapter-backed one, whose prompt waits for the provider thread) and each
    /// send accepted as `queued` while the launch was still running.
    /// [`SessionStore::open_sends`] is exactly that set, in id order.
    ///
    /// Must be called BEFORE [`Self::clean_up_failed_spawn_row`]: the rows
    /// cascade away with the session, and this frame is the last place their
    /// text exists.
    ///
    /// A read failure is logged and reported as "nothing outstanding" rather
    /// than propagated: the browser is waiting on a session that will never
    /// come up, and losing the failure report over a failed query would be the
    /// worse outcome.
    pub(in crate::interactor::lifecycle) async fn undelivered_sends(
        &self,
        session_id: &delta_model::SessionId,
    ) -> Vec<UnsentSend> {
        match self.store.open_sends(session_id).await {
            Ok(sends) => sends
                .into_iter()
                .map(|send| UnsentSend {
                    send_id: send.id,
                    text: send.text,
                })
                .collect(),
            Err(err) => {
                tracing::error!(
                    session_id = %session_id,
                    error = %err,
                    "failed to read the undelivered sends of a failed launch; \
                     reporting the failure without them (their text is lost)"
                );
                Vec::new()
            }
        }
    }

    /// Clean up the eagerly-created session row of a spawn that never bound.
    ///
    /// The row was INSERTed (status `spawning`) when the id was minted, before
    /// `claude` launched. A spawn that never bound ingested nothing, so the row
    /// — and its main thread plus every `send` row, removed by cascade — is
    /// deleted outright rather than kept as a `failed` tombstone. The user's
    /// text is not lost with them: the composer's Retry/Dismiss chip holds the
    /// FIRST prompt browser-side, and [`Self::undelivered_sends`] must run
    /// before this deletion to carry the rest out on the
    /// [`SessionEvent::SpawnFailed`] the caller emits. The `failed` status is
    /// kept only for the defensive case of a session that somehow already
    /// ingested messages (data worth keeping), which a never-bound spawn cannot
    /// normally reach.
    pub(in crate::interactor::lifecycle) async fn clean_up_failed_spawn_row(
        &self,
        session_id: &delta_model::SessionId,
    ) -> Result<()> {
        if self.store.message_count(session_id).await? == 0 {
            self.store.delete_session(session_id).await?;
        } else {
            self.store.mark_session_failed(session_id).await?;
        }
        Ok(())
    }
}
