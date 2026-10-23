use std::collections::BTreeSet;

use delta_model::Message;

use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{
    GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, TranscriptError, Workspace,
};
use crate::with_causes;

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
    ///
    /// **A failing transcript read is reported once, not on every tick.** A
    /// read that fails with a [`TranscriptError`] — refused, I/O,
    /// aborted — is answered with an empty batch, and warned about (with its
    /// cause chain) only when it is news: the first failure, one that differs
    /// from the last reported, or one after a read succeeded in between (that
    /// recovery is logged at `info`). A failure that may pass is simply tried
    /// again on the next tick. The record, and what counts as the same
    /// failure, live in the session's runtime state
    /// (`SessionRuntime::note_transcript_failure`). Any other failure (the
    /// store, say) fails this session's tick, which the fan-out reports for
    /// this session alone.
    pub(in crate::interactor) async fn sync_tick(
        &mut self,
    ) -> Result<(Vec<Message>, Vec<SessionEvent>)> {
        if !self.state.is_open() {
            return Ok((Vec::new(), Vec::new()));
        }
        // The session is open, so it must still be in the store; a missing
        // row would be a torn-down binding, so skip it defensively rather
        // than fail this session's tick on every poll.
        let Some(session) = self.store.session(self.id).await? else {
            return Ok((Vec::new(), Vec::new()));
        };
        let (messages, mut events) = match self.sync_transcript(&session).await {
            Ok(batch) => {
                self.note_transcript_read();
                batch
            }
            // A transcript read that fails tends to fail the same way on every
            // tick (a refusal until the file changes, a `permission denied`
            // until the mode does), so it is reported here, when it is news,
            // instead of failing the tick, which the fan-out would report on
            // every tick.
            Err(Error::Transcript(err)) => {
                self.note_transcript_failure(&err);
                return Ok((Vec::new(), Vec::new()));
            }
            Err(err) => return Err(err),
        };
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

    /// Report that the background tail could not read this session's
    /// transcript, unless the same failure was already reported (see
    /// `SessionRuntime::note_transcript_failure`).
    fn note_transcript_failure(&mut self, err: &TranscriptError) {
        let rendered = with_causes(err);
        if !self.state.note_transcript_failure(rendered.clone()) {
            return;
        }
        match err {
            TranscriptError::Refused { path, reason } => tracing::warn!(
                session_id = %self.id,
                path = %path.display(),
                reason = %reason,
                "the transcript is refused; the background tail skips this \
                 session and stays quiet until the refusal changes or a read \
                 succeeds"
            ),
            _ => tracing::warn!(
                session_id = %self.id,
                error = %rendered,
                "the transcript could not be read; the background tail retries \
                 on every tick and stays quiet until the failure changes or a \
                 read succeeds"
            ),
        }
    }

    /// Note that this session's transcript was read, logging the recovery
    /// when the background tail had reported it failing.
    fn note_transcript_read(&mut self) {
        if let Some(failure) = self.state.clear_transcript_failure() {
            tracing::info!(
                session_id = %self.id,
                was = %failure,
                "the transcript is read again after failing"
            );
        }
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
