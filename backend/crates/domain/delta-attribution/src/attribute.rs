//! The attribution fold: parsed transcript lines in, attributed messages and
//! effects out.

use std::collections::VecDeque;

use delta_model::{ContentBlock, Message, MessageUuid, Role, Send, SessionId, ThreadId};

use crate::claude_format;
use crate::transcript_message::TranscriptMessage;

/// The attribution-relevant view of one outstanding `dispatched` send: the
/// thread (and optional branch parent) its echo line must be attributed to,
/// and the text the echo is recognized by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutstandingSend {
    /// The send row id, echoed back in [`Effect::SendMatched`].
    pub id: i64,
    /// The thread this send is attributed to.
    pub thread_id: ThreadId,
    /// When branching, the message this reply is `to:`.
    pub semantic_parent_uuid: Option<MessageUuid>,
    /// The dispatched prompt text; the echo is matched by trimmed equality.
    pub text: String,
}

impl From<&Send> for OutstandingSend {
    fn from(send: &Send) -> Self {
        Self {
            id: send.id,
            thread_id: send.thread_id,
            semantic_parent_uuid: send.semantic_parent_uuid.clone(),
            text: send.text.clone(),
        }
    }
}

/// The state the fold threads from line to line (and the caller threads from
/// batch to batch). Seeding it from the store and folding a batch is exactly
/// equivalent to folding the same lines in any other batching: that is the
/// replay invariant the corpus tests pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionState {
    /// The thread of the turn in progress: the thread of the most recent user
    /// line, advanced by matched sends and reset to `main` by external input.
    /// Seeded from the latest persisted user message (defaulting to `main`).
    pub carry_thread: ThreadId,
    /// The outstanding `dispatched` sends in dispatch (FIFO) order. Only the
    /// head is ever compared — mirroring the store's `head_dispatched_send`,
    /// which always returns the oldest `dispatched` row — and a match consumes
    /// it, exposing the next.
    ///
    /// Under the single-outstanding dispatch rule a live session seeds at most
    /// one element here. The queue form is what makes whole-history replay
    /// work: seeding every send of a session in dispatch order folds the full
    /// transcript in one pass, each echo consuming its send in turn.
    pub outstanding: VecDeque<OutstandingSend>,
}

impl AttributionState {
    /// Seed a batch: the carry thread plus the at-most-one outstanding send.
    pub fn new(carry_thread: ThreadId, outstanding: Option<OutstandingSend>) -> Self {
        Self {
            carry_thread,
            outstanding: outstanding.into_iter().collect(),
        }
    }
}

/// A store/runtime action the fold decided on but cannot perform (it is
/// pure). The caller executes these **in order** after the fold returns; the
/// order is exactly the order the previous inline implementation performed
/// them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// A `tool_result` block was ingested: resolve the open permission
    /// request(s) correlated by this `tool_use_id`. `allowed` is inferred
    /// from the result's error flag (a denied tool yields `is_error: true`).
    ResolvePermission { tool_use_id: String, allowed: bool },
    /// The `[Request interrupted by user...]` marker was ingested: the user
    /// aborted the in-flight turn (Claude's `Stop` hook does not fire on
    /// interrupt). Feed `Interrupt` into the turn machine and notify the
    /// browser so the stuck pending send clears.
    TurnInterrupted,
    /// A human user line matched the head outstanding send: mark the send row
    /// matched to this transcript uuid.
    SendMatched {
        send_id: i64,
        matched_uuid: MessageUuid,
    },
}

/// The outcome of folding one batch of transcript lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attributed {
    /// The lines as attributed [`Message`]s, in input order.
    pub messages: Vec<Message>,
    /// The actions the caller must execute, in decision order.
    pub effects: Vec<Effect>,
    /// The state after the batch — the exact seed for folding the lines that
    /// follow.
    pub state: AttributionState,
}

/// Attribute a batch of parsed transcript lines to threads.
///
/// Attribution is driven by comparing a user line's trimmed text against the
/// head outstanding (`dispatched`) send — at most one exists under the
/// single-outstanding dispatch rule. Lines are processed in order while
/// maintaining `carry_thread`, the thread of the current turn:
///
/// - A **human** user line (a user line carrying author-written text) that
///   equals the head outstanding send's text is attributed to that send's
///   thread (the new child thread for a branch send), the send is consumed
///   (reported via [`Effect::SendMatched`]), and `carry_thread` advances to
///   it. A human user line matching no outstanding send is external input and
///   lands on `main`, resetting `carry_thread` — unless it is an uncorrelated
///   `queued_command`, a programmatic injection that inherits `carry_thread`.
/// - Every other line follows `carry_thread` — the thread of the turn it
///   belongs to. This covers assistant/system lines AND tool-result lines,
///   which Claude delivers as `role: user` but which are part of the
///   in-flight turn, not a new human turn. The interrupt marker is also a
///   `role: user` line belonging to the aborted turn: it inherits
///   `carry_thread` and additionally yields [`Effect::TurnInterrupted`].
pub fn attribute_lines(
    session_id: &SessionId,
    main_thread: ThreadId,
    mut state: AttributionState,
    lines: Vec<TranscriptMessage>,
) -> Attributed {
    let mut messages = Vec::with_capacity(lines.len());
    let mut effects = Vec::new();

    for line in lines {
        let content_text = Message::flatten_text(&line.content);

        // Correlate any tool_result blocks on this line with the open
        // permission requests they settle. Resolving on actual completion
        // (rather than at `PreToolUse` time) is what lets an auto-approved
        // tool's notice clear immediately while a genuine prompt's notice
        // persists until the human answers. A denied tool yields
        // `is_error: true` ("User rejected tool use"), so the error flag
        // infers allowed vs denied.
        for block in &line.content {
            if let ContentBlock::ToolResult {
                tool_use_id,
                is_error,
                ..
            } = block
            {
                effects.push(Effect::ResolvePermission {
                    tool_use_id: tool_use_id.clone(),
                    allowed: !is_error,
                });
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
        // must not run through send correlation nor reset to `main`).
        let trimmed = content_text.as_deref().unwrap_or("").trim();
        let is_interrupt_marker =
            matches!(line.role, Role::User) && claude_format::is_interrupt_marker(trimmed);
        let is_human_turn =
            matches!(line.role, Role::User) && !trimmed.is_empty() && !is_interrupt_marker;

        if is_interrupt_marker {
            effects.push(Effect::TurnInterrupted);
        }

        // Compare against the head outstanding send; a match consumes it.
        let (thread_id, semantic_parent_uuid) = if is_human_turn {
            let head_matches = state
                .outstanding
                .front()
                .is_some_and(|send| send.text.trim() == trimmed);
            match head_matches.then(|| state.outstanding.pop_front()).flatten() {
                Some(pending) => {
                    effects.push(Effect::SendMatched {
                        send_id: pending.id,
                        matched_uuid: line.uuid.clone(),
                    });
                    state.carry_thread = pending.thread_id;
                    (pending.thread_id, pending.semantic_parent_uuid)
                }
                None if line.is_queued_command => {
                    // A queued command with no matching send is a
                    // programmatic injection (e.g. a background task
                    // notification), not stray pane typing, so it must not
                    // tear the active turn back to `main` — inherit the
                    // current thread the way a non-human line does.
                    (state.carry_thread, None)
                }
                None => {
                    state.carry_thread = main_thread;
                    (main_thread, None)
                }
            }
        } else {
            (state.carry_thread, None)
        };

        messages.push(Message {
            uuid: line.uuid,
            session_id: session_id.clone(),
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

    Attributed {
        messages,
        effects,
        state,
    }
}
