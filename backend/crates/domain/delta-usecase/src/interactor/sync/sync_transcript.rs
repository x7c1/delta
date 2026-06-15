use delta_attribution::{attribute_lines, AttributionState, Effect, OutstandingSend};
use delta_model::{Message, Session};

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W> SessionContext<'_, T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Pull new transcript lines from disk and persist them as messages,
    /// attributing each to the right thread as it is ingested.
    ///
    /// Runs inside the session's actor, which is what serializes the
    /// read-cursor → read-file → ingest → set-cursor sequence: every caller
    /// (hook handlers, the background tail, open/close) reaches it through
    /// the same mailbox, so two syncs of one session can never interleave and
    /// double-ingest or race the cursor — and *different* sessions ingest in
    /// parallel, which the old global sync lock forbade. The cursor is
    /// per-session state, so per-session serialization is exactly enough.
    ///
    /// This method is only the I/O shell. The attribution decisions — which
    /// thread each line lands on, which send is consumed, which permission
    /// rows a tool_result settles, whether the turn was interrupted — are
    /// made by the pure fold [`delta_attribution::attribute_lines`], seeded
    /// here from the store (the latest persisted user thread as
    /// `carry_thread`, plus the at-most-one outstanding `dispatched` send)
    /// and executed afterwards as [`Effect`]s.
    ///
    /// Returns the newly-ingested messages and any [`SessionEvent`]s that the
    /// ingest produced. Two events can arise here:
    ///
    /// - [`SessionEvent::PermissionResolved`]: when a `tool_result` line is
    ///   ingested, the open permission request correlated by its `tool_use_id`
    ///   is resolved so the browser can clear the "permission requested" notice.
    /// - [`SessionEvent::TurnInterrupted`]: emitted in two cases, both of which
    ///   end a turn without firing Claude's `Stop` hook. (1) A `[Request
    ///   interrupted by user...]` marker line: the user aborted the in-flight
    ///   turn. (2) A synthetic `isApiErrorMessage` assistant line: the turn ended
    ///   on an API error (a usage/session limit, a rate limit, or any other API
    ///   failure). In either case this is the hook-independent signal that clears
    ///   the stuck send. They differ only in the turn-machine input fed (an
    ///   interrupt vs. a genuine stop), not in the browser signal.
    ///
    /// The caller is responsible for broadcasting these events.
    pub(in crate::interactor) async fn sync_transcript(
        &mut self,
        session: &Session,
    ) -> Result<(Vec<Message>, Vec<SessionEvent>)> {
        // A still-`spawning` session has no transcript path yet (the first hook
        // never bound it), so there is nothing to sync.
        let Some(transcript_path) = session.transcript_path.as_deref() else {
            return Ok((Vec::new(), Vec::new()));
        };
        let main_thread = self.store.main_thread_id(&session.id).await?;

        // Resume from the line-based cursor so each transcript line is read
        // exactly once. This is the file line index, not a message count: lines
        // that parse to nothing (blank, no-uuid such as Claude Code's
        // `file-history-snapshot`, or unparsable) still advance it, so the
        // cursor never lags behind the file and already-ingested lines are never
        // reprocessed.
        let from = self.store.transcript_lines_read(&session.id).await?;
        let read = self.transcript.read_from(transcript_path, from).await?;

        // Always advance the cursor to the file's true line count, even when no
        // new messages parsed, so skipped trailing lines are not re-read next
        // time.
        self.store
            .set_transcript_lines_read(&session.id, read.total_lines)
            .await?;

        if read.messages.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // Seed the fold: the turn in progress when this batch starts (the
        // thread of the most recent persisted user message, defaulting to
        // `main`) plus the one outstanding dispatched send, if any.
        let carry_thread = self
            .store
            .latest_user_thread(&session.id)
            .await?
            .unwrap_or(main_thread);
        let outstanding = self
            .store
            .head_dispatched_send(&session.id)
            .await?
            .as_ref()
            .map(OutstandingSend::from);
        let state = AttributionState::new(carry_thread, outstanding);

        let outcome = attribute_lines(&session.id, main_thread, state, read.messages);

        // Execute the fold's effects in decision order, then persist.
        let mut events = Vec::new();
        for effect in outcome.effects {
            match effect {
                Effect::ResolvePermission {
                    tool_use_id,
                    allowed,
                } => {
                    // Resolve the `PreToolUse`-recorded row keyed by
                    // `tool_use_id`, plus any pending dialog row the
                    // `PermissionRequest` hook owns (answered in the TUI after
                    // the browser-decision wait timed out).
                    for request_id in self
                        .store
                        .resolve_permission_by_tool_use_id(&session.id, &tool_use_id, allowed)
                        .await?
                    {
                        // Keep the queryable runtime mirror in step with the
                        // broadcast, so the sends envelope never reports a
                        // dialog (or question) that already resolved. The same
                        // `PermissionResolved` event clears either notice in
                        // the browser; a question's row id and a permission's
                        // row id are disjoint, so resolving both here is safe.
                        self.state.resolve_pending_permission(request_id);
                        self.state.resolve_pending_question(request_id);
                        events.push(SessionEvent::PermissionResolved {
                            session_id: session.id.clone(),
                            request_id,
                        });
                    }
                }
                Effect::TurnInterrupted => {
                    // The interrupt ends the turn: feed `Interrupt` into the
                    // turn machine (back to `Idle`). Dispatching any queued
                    // send is left to the caller (which acts on the returned
                    // `TurnInterrupted` after this sync returns), so no
                    // keystrokes are sent from inside the ingestion path.
                    self.apply_turn_input(crate::turn::TurnInput::Interrupt)
                        .await?;
                    events.push(SessionEvent::TurnInterrupted {
                        session_id: session.id.clone(),
                    });
                }
                Effect::TurnAborted => {
                    // A synthetic `isApiErrorMessage` line ended the turn on an
                    // API error (usage/session limit, rate limit, ...). The turn
                    // genuinely ended, so feed `Stop` into the turn machine (back
                    // to `Idle`) — this is the honest turn-end signal and gives
                    // the same orphan-send disposition the missing `Stop` hook
                    // would have. We reuse `TurnInterrupted` as the browser
                    // signal: like an interrupt, no `Stop` hook fired, so the
                    // browser must clear the stuck pending chip and drop any
                    // orphaned streaming preview (which may never get a persisted
                    // message). The caller releases the queued send after this
                    // sync returns (it keys on `TurnInterrupted`), so no
                    // keystrokes are sent from inside the ingestion path.
                    self.apply_turn_input(crate::turn::TurnInput::Stop).await?;
                    events.push(SessionEvent::TurnInterrupted {
                        session_id: session.id.clone(),
                    });
                }
                Effect::SendMatched {
                    send_id,
                    matched_uuid,
                } => {
                    self.store.mark_send_matched(send_id, &matched_uuid).await?;
                }
            }
        }

        self.store.upsert_messages(&outcome.messages).await?;
        Ok((outcome.messages, events))
    }
}
