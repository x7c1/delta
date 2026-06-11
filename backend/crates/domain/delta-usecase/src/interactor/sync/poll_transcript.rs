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
    /// Poll the transcript of every currently-open (live-pane) session for
    /// newly-written lines.
    ///
    /// Drives the continuous background tail: Claude Code often flushes the final
    /// assistant line to the JSONL *after* the `Stop` hook fires, so the hook's
    /// sync misses it and the reply never reaches the browser until the next
    /// hook. Polling on an interval ingests those late lines and returns them so
    /// the caller can announce the transcript growth.
    ///
    /// **Scoped to open sessions only.** The late-line, interrupt, and
    /// deferred-send releases this tail catches can only happen on a session
    /// Delta is actively running, so it iterates the open-session registry
    /// (sessions with a live pane) rather than the whole store. This keeps the
    /// tail's cost proportional to the number of concurrently-open sessions
    /// instead of O(total history) — every session that ever existed would
    /// otherwise be re-synced on every tick. It also stops an *external* resume
    /// of a closed session (`claude --resume <id>` outside Delta) from growing
    /// the shared on-disk JSONL and having that growth ingested and streamed into
    /// Delta's UI for a session Delta holds no pane for. The last line of a
    /// session is captured by a final sync on [`Self::close_session`], just
    /// before it leaves the open set.
    ///
    /// Each session is synced independently and the result is grouped by session:
    /// one entry per session that ingested new messages, in arbitrary order. A
    /// quiet open session simply yields no new lines and is omitted, so every
    /// returned group is non-empty — callers may index `group[0]` for the
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
    /// when no session is open.
    pub async fn poll_transcript(&self) -> Result<(Vec<Vec<Message>>, Vec<SessionEvent>)> {
        let open_ids = self.open_sessions.lock().await.open_session_ids();
        let mut groups = Vec::new();
        let mut events = Vec::new();
        for id in open_ids {
            // The session is open, so it must still be in the store; a missing
            // row would be a torn-down registry entry, so skip it defensively
            // rather than error the whole tick.
            let Some(session) = self.store.session(&id).await? else {
                continue;
            };
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
