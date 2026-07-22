//! The provider-neutral conversation-content seam.
//!
//! A [`ConversationSource`] is the second seam an agent provider exposes,
//! parallel to the adapter's lossy control-loop `events()` stream. Where
//! `events()` carries the coarse turn/permission facts the FSM needs, a
//! `ConversationSource` carries the lossless *canonical conversation content*:
//! a batch of newly-observed [`Message`]s plus the ordered [`Effect`]s a
//! provider-neutral persistence pipeline must execute. The two layers are kept
//! deliberately separate — squeezing content through the event enum would drop
//! the fidelity (thinking, tool-use ids, per-message model/seq, both parent
//! links) the message history depends on.
//!
//! Claude Code's producer is [`ClaudeConversationSource`]: it wraps the JSONL
//! transcript read and the pure attribution fold
//! ([`delta_attribution::attribute_lines`]). Every Claude-specific concern —
//! the line-based cursor, the [`AttributionState`] seeding (carry thread,
//! single-outstanding-send correlation, background-launch map) — stays inside
//! this producer and never leaks into the neutral pipeline that consumes the
//! batch, so a future Codex producer can supply the same `(messages, effects)`
//! shape from an entirely different transport.

use async_trait::async_trait;
use delta_attribution::{attribute_lines, AttributionState, Effect, OutstandingSend};
use delta_model::{Message, Session};

use crate::error::Result;
use crate::ports::{SessionStore, Transcript};

/// A per-provider producer of canonical conversation content.
///
/// Yields, for a batch of newly-observed provider output, the attributed
/// [`Message`]s and the ordered [`Effect`]s the neutral persistence pipeline
/// must execute. Provider-specific attribution/accumulation state lives inside
/// the implementation; the returned batch is provider-neutral.
#[async_trait]
pub(in crate::interactor) trait ConversationSource {
    /// Produce the next batch of canonical conversation content: the messages
    /// newly attributed in this window and the effects the pipeline must
    /// execute, in decision order. When there is no new provider content the
    /// batch is empty (`(vec![], vec![])`).
    async fn next_batch(&mut self, session: &Session) -> Result<(Vec<Message>, Vec<Effect>)>;
}

/// Claude Code's [`ConversationSource`]: the JSONL transcript read plus the
/// pure attribution fold.
///
/// Holds only borrows of the transcript reader and the store — it owns no
/// state of its own between batches, because the fold's cross-batch state
/// (the cursor, the outstanding sends, the background-launch map) is persisted
/// in the store and reseeded on every call. That is exactly what makes the
/// fold replayable (see [`delta_attribution`]).
pub(in crate::interactor) struct ClaudeConversationSource<'a, X, S> {
    transcript: &'a X,
    store: &'a S,
}

impl<'a, X, S> ClaudeConversationSource<'a, X, S> {
    pub(in crate::interactor) fn new(transcript: &'a X, store: &'a S) -> Self {
        Self { transcript, store }
    }
}

#[async_trait]
impl<X, S> ConversationSource for ClaudeConversationSource<'_, X, S>
where
    X: Transcript,
    S: SessionStore,
{
    async fn next_batch(&mut self, session: &Session) -> Result<(Vec<Message>, Vec<Effect>)> {
        // A still-`spawning` session has no transcript path yet (the first hook
        // never bound it), so there is nothing to source.
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
        // Reseed the outstanding background-task launches: a `run_in_background`
        // Agent/Task/Bash launched in an earlier sync window, whose completion
        // notification may land in this one. Without this the notification would
        // not find its launching thread and fall back to `carry_thread`.
        let launches = self
            .store
            .outstanding_subagent_launches(&session.id)
            .await?;
        let state = AttributionState::with_launches(carry_thread, outstanding, launches);

        let outcome = attribute_lines(&session.id, main_thread, state, read.messages);
        Ok((outcome.messages, outcome.effects))
    }
}
