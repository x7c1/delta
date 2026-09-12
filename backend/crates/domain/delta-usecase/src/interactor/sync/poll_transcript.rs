use std::collections::BTreeSet;

use delta_model::Message;

use crate::error::Result;
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
    /// One background-tail tick for this session: poll its transcript for
    /// newly-written lines, if it is open.
    ///
    /// Drives the continuous background tail (fanned out per session by the
    /// interactor's `poll_transcript`): Claude Code often flushes the final
    /// assistant line to the JSONL *after* the `Stop` hook fires, so the
    /// hook's sync misses it. Polling on an interval ingests those late lines
    /// and announces the transcript growth (see below).
    ///
    /// **A no-op for a session with no live pane.** The late-line, interrupt,
    /// and queued-send releases this tail catches can only happen on a session
    /// Delta is actively running, so the tail's cost stays proportional to the
    /// number of concurrently-open sessions rather than the total history. It
    /// also stops an *external* resume of a closed session (`claude --resume
    /// <id>` outside Delta) from growing the shared on-disk JSONL and having
    /// that growth ingested and streamed into Delta's UI for a session Delta
    /// holds no pane for. The last line of a session is captured by a final
    /// sync on `close_session`, just before its pane is dropped.
    ///
    /// **This session announces its own ingest.** As soon as the batch is
    /// persisted, a [`SessionEvent::TranscriptUpdated`] for the threads it
    /// landed on — and every other [`SessionEvent`] the ingest produced (e.g.
    /// [`SessionEvent::PermissionResolved`] when a late `tool_result` is
    /// tailed in; most tool_results are ingested here by the continuous tail,
    /// so this is the primary path that clears an auto-approved tool's
    /// notice) — is pushed onto the interactor's async event seam, which the
    /// server drains into its broadcast. Emitting from inside the mailbox that
    /// serialized the ingest means the browser learns of the new lines at the
    /// moment the rows land, rather than after *every* session has answered
    /// the tick: one slow actor can no longer silence every other session.
    ///
    /// The same messages and events are also *returned*, unchanged, for
    /// callers that inspect the batch. They have already been emitted, so a
    /// caller that drains the seam must not broadcast them a second time.
    pub(in crate::interactor) async fn sync_tick(
        &mut self,
    ) -> Result<(Vec<Message>, Vec<SessionEvent>)> {
        if !self.state.is_open() {
            return Ok((Vec::new(), Vec::new()));
        }
        // The session is open, so it must still be in the store; a missing
        // row would be a torn-down binding, so skip it defensively rather
        // than error the whole tick.
        let Some(session) = self.store.session(self.id).await? else {
            return Ok((Vec::new(), Vec::new()));
        };
        let (messages, mut events) = self.sync_transcript(&session).await?;
        // An interrupt ends the turn but fires no `Stop` hook, so the tail is
        // where it is observed. Release any queued send now that the session
        // is idle — done here, after `sync_transcript` has returned, so
        // dispatching sends no keystrokes from inside the ingestion path.
        let interrupted = events.iter().any(
            |e| matches!(e, SessionEvent::TurnInterrupted { session_id, .. } if session_id == self.id),
        );
        if interrupted {
            // A dispatch failure is logged rather than propagated, mirroring
            // the resume and echo-deadline ticks: `dispatch_queued_send` has
            // already cancelled the row it failed on, so failing the tick buys
            // no recovery. It would cost the announcement below — and these
            // lines are persisted, so no later tick re-offers them and the
            // browser would never refetch what this one ingested.
            match self.dispatch_queued_send().await {
                Ok(Some(event)) => events.push(event),
                Ok(None) => {}
                Err(err) => tracing::warn!(
                    session_id = %self.id,
                    error = %err,
                    "failed to release a queued send after a tailed-in interrupt"
                ),
            }
        }
        self.announce_ingest(&messages, &events);
        Ok((messages, events))
    }

    /// Announce what this tick ingested on the async event seam.
    ///
    /// The `TranscriptUpdated` carries the distinct threads the batch landed
    /// on, so the browser refetches exactly those; the ingest's other events
    /// follow it, in the order they were produced. Nothing is emitted for an
    /// empty batch with no events — the common case on a quiet tick.
    ///
    /// The `debug` line is the per-session record the tail previously lacked:
    /// when a session looks silent, the log shows which sessions ingested how
    /// much, and when.
    fn announce_ingest(&self, messages: &[Message], events: &[SessionEvent]) {
        if !messages.is_empty() {
            let thread_ids: Vec<_> = messages
                .iter()
                .map(|m| m.thread_id)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            tracing::debug!(
                session_id = %self.id,
                ingested = messages.len(),
                threads = thread_ids.len(),
                "background tail ingested new transcript lines"
            );
            self.emit_async_event(SessionEvent::TranscriptUpdated {
                session_id: self.id.clone(),
                thread_ids,
            });
        }
        for event in events {
            self.emit_async_event(event.clone());
        }
    }
}
