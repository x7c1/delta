use delta_model::Message;

use crate::error::Result;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Poll every registered session's transcript for newly-written lines.
    ///
    /// Drives the continuous background tail: Claude Code often flushes the final
    /// assistant line to the JSONL *after* the `Stop` hook fires, so the hook's
    /// sync misses it and the reply never reaches the browser until the next
    /// hook. Polling on an interval ingests those late lines and returns them so
    /// the caller can announce the transcript growth.
    ///
    /// Each session is synced independently and the result is grouped by session:
    /// one entry per session that ingested new messages, in registration order.
    /// A closed or quiet session simply yields no new lines and is omitted, so
    /// every returned group is non-empty — callers may index `group[0]` for the
    /// group's session id. This lets the caller emit one transcript-growth
    /// notification per session.
    ///
    /// Reuses [`Self::sync_transcript`] (cursor, attribution, the serialization
    /// lock), so it is safe to call concurrently with the hook handlers.
    ///
    /// Alongside the per-session message groups, returns any [`SessionEvent`]s
    /// the ingest produced (e.g. [`SessionEvent::PermissionResolved`] when a
    /// late `tool_result` is tailed in) for the caller to broadcast. Most
    /// tool_results are ingested here by the continuous tail, so this is the
    /// primary path that clears an auto-approved tool's notice. Returns empty
    /// when no session has been registered yet.
    pub async fn poll_transcript(&self) -> Result<(Vec<Vec<Message>>, Vec<SessionEvent>)> {
        let mut groups = Vec::new();
        let mut events = Vec::new();
        for session in self.store.list_sessions().await? {
            let (messages, resolved_events) = self.sync_transcript(&session).await?;
            // An interrupt ends the turn but fires no `Stop` hook, so the tail is
            // where it is observed. Release any deferred send now that the
            // session is idle — done here, after `sync_transcript` has returned
            // and dropped its lock, so dispatching sends no keystrokes from
            // inside the ingestion path.
            let interrupted = resolved_events.iter().any(|e| {
                matches!(e, SessionEvent::TurnInterrupted { session_id } if *session_id == session.id)
            });
            events.extend(resolved_events);
            if !messages.is_empty() {
                groups.push(messages);
            }
            if interrupted {
                self.dispatch_deferred_send(&session.id).await?;
            }
        }
        Ok((groups, events))
    }
}
