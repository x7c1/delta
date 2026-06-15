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
    /// and returns them so the caller can announce the transcript growth.
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
    /// Alongside the newly-ingested messages, returns any [`SessionEvent`]s
    /// the ingest produced (e.g. [`SessionEvent::PermissionResolved`] when a
    /// late `tool_result` is tailed in) for the caller to broadcast. Most
    /// tool_results are ingested here by the continuous tail, so this is the
    /// primary path that clears an auto-approved tool's notice.
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
            |e| matches!(e, SessionEvent::TurnInterrupted { session_id } if session_id == self.id),
        );
        if interrupted {
            if let Some(event) = self.dispatch_queued_send().await? {
                events.push(event);
            }
        }
        Ok((messages, events))
    }
}
