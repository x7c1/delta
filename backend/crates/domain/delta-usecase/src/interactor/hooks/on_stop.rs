use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{SessionEvent, SessionStore, StopHook, TmuxDriver, Transcript, Workspace};
use crate::turn::TurnInput;

impl<T, X, S, W> SessionContext<'_, T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Handle a `Stop` hook: ingest the final transcript lines and report the
    /// turn as completed.
    pub(in crate::interactor) async fn on_stop(&mut self, hook: StopHook) -> Result<Vec<SessionEvent>> {
        let mut events = Vec::new();
        // The hook was routed here by its own session id, so the right
        // session's transcript is synced even when several sessions are
        // registered. The final transcript lines often include the last
        // tool_result, so the `Stop` sync is a key place permission requests
        // resolve.
        if let Some(session) = self.store.session(&hook.session_id).await? {
            let (_messages, resolved_events) = self.sync_transcript(&session).await?;
            events.extend(resolved_events);
        }
        // The turn ended: feed `Stop` into the turn machine (back to `Idle`),
        // then release the next queued send — one at a time, the
        // single-outstanding rule — now that the session is idle. Dispatching
        // it moves the machine to `AwaitingEcho` for its own turn.
        self.apply_turn_input(TurnInput::Stop).await?;
        if let Some(event) = self.dispatch_queued_send().await? {
            events.push(event);
        }
        events.push(SessionEvent::TurnCompleted {
            session_id: hook.session_id,
            stop_reason: hook.stop_reason,
        });
        Ok(events)
    }
}
