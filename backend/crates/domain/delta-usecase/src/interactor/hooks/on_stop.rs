use crate::agent::TurnStatus;
use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{
    GitWorktree, SessionEvent, SessionStore, StopHook, TmuxDriver, Transcript, Workspace,
};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Handle a `Stop` hook: ingest the final transcript lines and report the
    /// turn as completed.
    pub(in crate::interactor) async fn on_stop(
        &mut self,
        hook: StopHook,
    ) -> Result<Vec<SessionEvent>> {
        let mut events = Vec::new();
        // The hook was routed here by its own session id, so the right
        // session's transcript is synced even when several sessions are
        // registered. The final transcript lines often include the last
        // tool_result, so the `Stop` sync is a key place permission requests
        // resolve.
        //
        // Recover the in-flight turn's thread BEFORE the turn machine runs:
        // `apply_turn_input(Stop)` can sweep the head dispatched send (the
        // authoritative thread source), so resolving afterwards would lose it.
        // Only resolve when the session is registered — a `Stop` for a session
        // Delta never saw has no thread to resolve (and `main_thread_id` has no
        // row to read), so its degenerate broadcast carries `None`.
        let thread_id = if let Some(session) = self.store.session(&hook.session_id).await? {
            let (_messages, resolved_events) = self.sync_transcript(&session).await?;
            events.extend(resolved_events);
            Some(self.store.in_progress_turn_thread(&hook.session_id).await?)
        } else {
            None
        };
        // The turn ended: the `Stop` hook is Claude's honest turn-completion
        // signal, so route it as a `TurnCompleted(Completed)` fact (which maps
        // to the machine's `Stop` input, back to `Idle`), then release the next
        // queued send — one at a time, the single-outstanding rule — now that
        // the session is idle. Dispatching it moves the machine to
        // `AwaitingEcho` for its own turn.
        self.apply_turn_end(TurnStatus::Completed).await?;
        if let Some(event) = self.dispatch_queued_send().await? {
            events.push(event);
        }
        events.push(SessionEvent::TurnCompleted {
            session_id: hook.session_id,
            thread_id,
            stop_reason: hook.stop_reason,
        });
        Ok(events)
    }
}
