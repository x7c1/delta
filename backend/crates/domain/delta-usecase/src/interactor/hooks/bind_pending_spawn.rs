use delta_model::Session;

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::input::SessionInput;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Idempotently bind this session's fresh spawn and activate its
    /// eagerly-created session row, returning the activated session when a
    /// bind happened here.
    ///
    /// This is the single binding step shared by the two signals that can first
    /// contact a Delta spawn: `SessionStart(source=startup)` and the first
    /// `UserPromptSubmit`. Whichever arrives first does the real work; the other
    /// is a no-op. Concretely, it activates the session row written eagerly at
    /// spawn time — `spawning` → `active`, filling in the hook-reported
    /// transcript path that was unknown when the id was minted — emits
    /// [`SessionEvent::SessionRegistered`], and only then moves the recorded
    /// [`PendingSpawn`] into the bound pane (via
    /// [`SessionRuntime::bind_pending_spawn`]). Any first prompt's `send` row
    /// was already written at spawn time, so no row writing happens at bind.
    ///
    /// **The runtime transition is deliberately last.** Registration is
    /// fallible — it validates the hook-reported transcript path and writes the
    /// row — and a spawn consumed before that failure would be lost: every
    /// later hook would find nothing pending, fall back to the still-`spawning`
    /// row with a `NULL` transcript path, and the session would be wedged with
    /// a bound pane, no transcript to tail, and no retry. Registering first
    /// instead leaves the [`PendingSpawn`] in place when it fails, so the next
    /// hook for this id retries the whole bind, and the stale-pending sweep
    /// still reports the launch as failed if no hook ever succeeds.
    ///
    /// It then posts a [`SessionInput::FlushQueuedSend`] to this actor's own
    /// mailbox. A session accepts sends as `queued` rows for as long as it is
    /// spawning, and binding is the moment they become dispatchable — but this
    /// runs inside a hook that blocks `claude` until its handler returns, so
    /// typing here would put the keystrokes into a TUI that is not yet
    /// accepting input and lose them. Posting moves the dispatch to the next
    /// mailbox iteration, after the hook has returned. A spawn that carried a
    /// first prompt is `AwaitingEcho` at bind, so the posted flush is a no-op
    /// there and the queue drains at that turn's `Stop`; the flush is what
    /// covers the prompt-less spawn, which binds idle with no `Stop` coming.
    ///
    /// Returns:
    /// - `Ok(Some(session))` — this call bound the pending spawn and activated it.
    /// - `Ok(None)` — nothing was pending (already bound by a prior call, or
    ///   the id belongs to an external/unknown session). A no-op: the caller
    ///   decides what to do with an unmatched id.
    /// - `Err(_)` — registration was refused (e.g. an out-of-root transcript
    ///   path). Nothing was bound and the spawn stays pending for the next
    ///   hook.
    ///
    /// [`PendingSpawn`]: crate::interactor::session_actor::runtime::PendingSpawn
    /// [`SessionRuntime::bind_pending_spawn`]: crate::interactor::session_actor::runtime::SessionRuntime::bind_pending_spawn
    pub(in crate::interactor::hooks) async fn bind_pending_spawn(
        &mut self,
        cwd: &str,
        transcript_path: &str,
        events: &mut Vec<SessionEvent>,
    ) -> Result<Option<Session>> {
        // Look without consuming: nothing pending means the spawn was already
        // bound (idempotent re-entry) or this is an external/unknown id.
        if !self.state.has_pending_spawn() {
            return Ok(None);
        }

        // Fallible work first — a refused path (or a failed write) must leave
        // the spawn pending so the next hook retries. See the doc comment.
        let (session, _main_id) = self
            .core
            .register_session_row(self.id, cwd, transcript_path, events)
            .await?;
        // Registered: now the runtime transition, which cannot fail. Nothing
        // can slip between the look and the take — the actor holds its state
        // by `&mut` across the await above, so no other input is handled in
        // between — and `Ok(Some(..))` below promises a bind really happened.
        let bound = self.state.bind_pending_spawn();
        debug_assert!(
            bound,
            "a spawn that was pending before the await is still pending after it"
        );
        // Deliberately posted, not called: see the doc comment above. Weak,
        // like every self-post, so a retired actor simply drops it.
        if let Some(sender) = self.self_sender.upgrade() {
            let _ = sender.send(SessionInput::FlushQueuedSend);
        }
        Ok(Some(session))
    }
}
