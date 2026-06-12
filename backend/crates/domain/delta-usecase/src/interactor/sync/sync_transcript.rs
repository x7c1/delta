use delta_model::{Message, Session};

use crate::error::Result;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

/// Prefix Claude Code writes to the transcript when the user interrupts the
/// in-flight turn. It appears as a `role: user` line whose only text block is
/// either `[Request interrupted by user]` (plain mid-response interrupt) or
/// `[Request interrupted by user for tool use]` (interrupt during a tool use).
/// Matching on the shared prefix covers both variants (and any future suffix)
/// without enumerating each exact string.
const INTERRUPT_MARKER_PREFIX: &str = "[Request interrupted by user";

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Pull new transcript lines from disk and persist them as messages,
    /// attributing each to the right thread as it is ingested.
    ///
    /// Attribution is driven by matching a user line's trimmed text to a queued
    /// `send`, so it is robust regardless of which hook triggered the
    /// sync or whether the line was present when `UserPromptSubmit` fired.
    /// Lines are processed in order while maintaining `carry_thread`, the thread
    /// of the current turn:
    ///
    /// - A **human** user line (a user line carrying author-written text) that
    ///   matches a still-`pending` send is attributed to that send's thread (the
    ///   new child thread for a branch send), the send is marked matched, and
    ///   `carry_thread` advances to it. A human user line with no matching send
    ///   is external input and lands on `main`, resetting `carry_thread`.
    /// - Every other line follows `carry_thread` — the thread of the turn it
    ///   belongs to. This covers assistant/system lines AND tool-result lines,
    ///   which Claude delivers as `role: user` but which are part of the
    ///   in-flight turn, not a new human turn.
    ///
    /// Returns the newly-ingested messages and any [`SessionEvent`]s that the
    /// ingest produced. Two events can arise here:
    ///
    /// - [`SessionEvent::PermissionResolved`]: when a `tool_result` line is
    ///   ingested, the open permission request correlated by its `tool_use_id`
    ///   is resolved so the browser can clear the "permission requested" notice.
    /// - [`SessionEvent::TurnInterrupted`]: when a `[Request interrupted by
    ///   user...]` marker line is ingested, signalling the user aborted the
    ///   in-flight turn. Claude's `Stop` hook does not fire on interrupt, so this
    ///   is the hook-independent signal that clears the stuck pending send.
    ///
    /// The caller is responsible for broadcasting these events.
    pub(in crate::interactor) async fn sync_transcript(
        &self,
        session: &Session,
    ) -> Result<(Vec<Message>, Vec<SessionEvent>)> {
        // Serialize the whole cursor → read → ingest → cursor sequence so the
        // hook handlers and the background tail cannot interleave and
        // double-ingest or race the cursor (see `sync_lock`).
        let _guard = self.sync_lock.lock().await;

        let transcript_path = &session.transcript_path;
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

        // The turn in progress when this batch starts: the thread of the most
        // recent persisted user message, defaulting to `main`.
        let mut carry_thread = self
            .store
            .latest_user_thread(&session.id)
            .await?
            .unwrap_or(main_thread);

        let mut messages = Vec::with_capacity(read.messages.len());
        let mut events = Vec::new();
        for line in read.messages {
            let content_text = Message::flatten_text(&line.content);

            // Correlate any tool_result blocks on this line with an open
            // permission request keyed by `tool_use_id`. Resolving on actual
            // completion (rather than at `PreToolUse` time) is what lets an
            // auto-approved tool's notice clear immediately while a genuine TUI
            // prompt's notice persists until the human answers. A denied tool
            // yields `is_error: true` ("User rejected tool use"), so the error
            // flag infers allowed vs denied.
            for block in &line.content {
                if let delta_model::ContentBlock::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } = block
                {
                    if let Some(request_id) = self
                        .store
                        .resolve_permission_by_tool_use_id(&session.id, tool_use_id, !is_error)
                        .await?
                    {
                        events.push(SessionEvent::PermissionResolved {
                            session_id: session.id.clone(),
                            request_id,
                        });
                    }
                }
            }

            // A genuine human turn is a user line with author-written text.
            // Claude delivers tool results as `role: user` lines too, but those
            // belong to the in-flight turn, not a new human turn, so they must
            // inherit `carry_thread` rather than reset it to `main`. (Mirrors the
            // frontend's `isUserTurn`.) Treating a tool_result as a turn boundary
            // used to drop the rest of a sub-thread's turn onto `main`.
            //
            // An interrupt marker is also a `role: user` line, but it belongs to
            // the turn the user just aborted, not a new human turn — so it too
            // inherits `carry_thread` and is excluded from `is_human_turn` (it
            // must not run through `match_dispatched_send` nor reset to `main`).
            let trimmed = content_text.as_deref().unwrap_or("").trim();
            let is_interrupt_marker = matches!(line.role, delta_model::Role::User)
                && trimmed.starts_with(INTERRUPT_MARKER_PREFIX);
            let is_human_turn = matches!(line.role, delta_model::Role::User)
                && !trimmed.is_empty()
                && !is_interrupt_marker;

            // The interrupt is hook-independent (Claude's `Stop` hook does not
            // fire on interrupt), so emit `TurnInterrupted` here to clear the
            // stuck pending send in the browser.
            //
            // It also ends the turn, so clear the in-flight flag. Dispatching any
            // queued send is left to the caller (which acts on the returned
            // `TurnInterrupted` once this sync's lock is released), so no
            // keystrokes are sent from inside the ingestion path.
            if is_interrupt_marker {
                self.store.set_turn_active(&session.id, false).await?;
                events.push(SessionEvent::TurnInterrupted {
                    session_id: session.id.clone(),
                });
            }

            let (thread_id, semantic_parent_uuid) = if is_human_turn {
                match self.store.match_dispatched_send(&session.id, trimmed).await? {
                    Some(pending) => {
                        self.store.mark_send_matched(pending.id, &line.uuid).await?;
                        carry_thread = pending.thread_id;
                        (pending.thread_id, pending.semantic_parent_uuid)
                    }
                    None if line.is_queued_command => {
                        // A queued command with no matching send is a
                        // programmatic injection (e.g. a background task
                        // notification), not stray pane typing, so it must not
                        // tear the active turn back to `main` — inherit the
                        // current thread the way a non-human line does.
                        (carry_thread, None)
                    }
                    None => {
                        carry_thread = main_thread;
                        (main_thread, None)
                    }
                }
            } else {
                (carry_thread, None)
            };

            messages.push(Message {
                uuid: line.uuid,
                session_id: session.id.clone(),
                thread_id,
                role: line.role,
                linear_parent_uuid: line.linear_parent_uuid,
                semantic_parent_uuid,
                prompt_id: line.prompt_id,
                // Persist the message's own transcript line index as its `seq`,
                // so ordering follows true file position with no drift.
                seq: line.seq,
                content_text,
                content: line.content,
                created_at: line.created_at,
            });
        }

        self.store.upsert_messages(&messages).await?;
        Ok((messages, events))
    }
}
