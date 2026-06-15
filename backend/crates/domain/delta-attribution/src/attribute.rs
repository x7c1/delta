//! The attribution fold: parsed transcript lines in, attributed messages and
//! effects out.

use std::collections::{BTreeMap, VecDeque};

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
    /// The launching thread of each outstanding background task, keyed by the
    /// launching tool_use `id` (the `toolu_...` value). A background subagent
    /// or background Bash (`run_in_background: true`) returns immediately and
    /// its completion is injected later as a `<task-notification>` user line
    /// carrying that same id in its `<tool-use-id>` element. Looking the id up
    /// here attributes the notification (and the assistant continuation it
    /// drives) to the thread that LAUNCHED the task, instead of blindly
    /// inheriting whatever thread is current when it lands — which is wrong
    /// whenever the user moved threads while the task ran.
    ///
    /// A map (not a single head) because several background tasks — launched
    /// from different threads, possibly nested — can be outstanding at once,
    /// and a completion must find its own launch by id. Like `outstanding`,
    /// this survives across sync windows by being seeded from a persisted
    /// store at batch start and mutated through effects: a launch is recorded
    /// ([`Effect::SubagentLaunched`]) when first seen and cleared
    /// ([`Effect::SubagentCompleted`]) when its notification is folded.
    /// `BTreeMap` keeps the seed-from-store ↔ fold round-trip deterministic.
    pub launched_threads: BTreeMap<String, ThreadId>,
}

impl AttributionState {
    /// Seed a batch: the carry thread plus the at-most-one outstanding send.
    /// The launch map starts empty; use [`Self::with_launches`] to seed it from
    /// the persisted background-launch store.
    pub fn new(carry_thread: ThreadId, outstanding: Option<OutstandingSend>) -> Self {
        Self {
            carry_thread,
            outstanding: outstanding.into_iter().collect(),
            launched_threads: BTreeMap::new(),
        }
    }

    /// Seed a batch with the outstanding background-launch map alongside the
    /// carry thread and outstanding send. The map carries `(tool_use_id ->
    /// launching thread)` for every background task still awaiting its
    /// `<task-notification>`.
    pub fn with_launches(
        carry_thread: ThreadId,
        outstanding: Option<OutstandingSend>,
        launched_threads: BTreeMap<String, ThreadId>,
    ) -> Self {
        Self {
            launched_threads,
            ..Self::new(carry_thread, outstanding)
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
    /// browser so the stuck send clears.
    TurnInterrupted,
    /// A synthetic `isApiErrorMessage` assistant line was ingested: the turn
    /// ended on an API error (a usage/session limit, a rate limit, or any other
    /// API failure) rather than completing normally. Like an interrupt, this
    /// turn-end fires **no** `Stop` hook and writes **no** interrupt marker, so
    /// without this effect the turn machine would stay in flight forever and
    /// every later send would defer to `queued` and never dispatch. Feed the
    /// turn machine back to idle and notify the browser so the stuck send
    /// clears. Detected from the structural flag, never the error text, so it
    /// covers every synthetic API-error turn-end and is locale-independent.
    TurnAborted,
    /// A human user line matched the head outstanding send: mark the send row
    /// matched to this transcript uuid.
    SendMatched {
        send_id: i64,
        matched_uuid: MessageUuid,
    },
    /// A background task (`run_in_background: true` Agent/Task/Bash) was first
    /// seen launching on an assistant line: persist `(tool_use_id ->
    /// thread_id)` so its later `<task-notification>` — which may arrive in a
    /// different sync window — can be attributed back to the launching thread.
    SubagentLaunched {
        tool_use_id: String,
        thread_id: ThreadId,
    },
    /// A background task's `<task-notification>` was folded and matched a
    /// recorded launch: clear the persisted `(tool_use_id -> thread_id)`
    /// correlation now that it has been consumed.
    SubagentCompleted { tool_use_id: String },
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
///   `carry_thread` and additionally yields [`Effect::TurnInterrupted`]. A
///   synthetic `isApiErrorMessage` assistant line (a turn that ended on a
///   usage/session limit, a rate limit, or any other API error) likewise
///   inherits `carry_thread` and additionally yields [`Effect::TurnAborted`],
///   the turn-end signal it carries in place of the absent `Stop` hook /
///   interrupt marker. A `<task-notification>` (a harness-injected
///   background-task completion, delivered as a plain `role: user` line) is a
///   programmatic continuation, so it never resets to `main`. It is attributed
///   to the thread that LAUNCHED the task: its `<tool-use-id>` is looked up in
///   `launched_threads` (recorded when the background `Agent`/`Task`/`Bash`
///   tool_use was first seen), so the completion lands on the launching thread
///   even when the user has moved to a different thread while the task ran.
///   Only when the id is absent from the map (the launch fell in an earlier,
///   no-longer-seeded window) does it fall back to inheriting `carry_thread`.
///
/// Whenever an assistant line carries a background `Agent`/`Task`/`Bash`
/// tool_use (`run_in_background: true`), its `id` is recorded against the
/// current `carry_thread` (the launching thread) and emitted as
/// [`Effect::SubagentLaunched`] for the caller to persist; the matching
/// notification later clears it via [`Effect::SubagentCompleted`].
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
            match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } => {
                    effects.push(Effect::ResolvePermission {
                        tool_use_id: tool_use_id.clone(),
                        allowed: !is_error,
                    });
                }
                // A background `Agent`/`Task`/`Bash` (`run_in_background: true`)
                // returns immediately; its completion is injected later as a
                // `<task-notification>` carrying this same `id`. Record
                // `(tool_use_id -> launching thread)` so that notification —
                // possibly in a later sync window — is attributed to the thread
                // that launched it rather than whatever thread is current then.
                // The launching thread is `carry_thread`: a tool_use is part of
                // the in-flight turn, whose thread `carry_thread` already holds.
                ContentBlock::ToolUse { id, input, .. }
                    if claude_format::launches_in_background(input) =>
                {
                    state
                        .launched_threads
                        .insert(id.clone(), state.carry_thread);
                    effects.push(Effect::SubagentLaunched {
                        tool_use_id: id.clone(),
                        thread_id: state.carry_thread,
                    });
                }
                _ => {}
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
        //
        // A `<task-notification>` is a third such `role: user` line: the
        // harness injects it to report a background task's completion, so it is
        // a programmatic continuation of the in-flight turn, not a new human
        // turn. Claude delivers it as a normal `type: "user"` line (NOT a
        // legacy `queued_command` attachment), so the parser does not flag it
        // `is_queued_command`. It must likewise be excluded from
        // `is_human_turn` and inherit `carry_thread`; otherwise, when a
        // background task completes while the user is working in a sub-thread,
        // the notification (and the assistant's continuation, and every later
        // turn) would reset to `main`.
        let trimmed = content_text.as_deref().unwrap_or("").trim();
        let is_interrupt_marker =
            matches!(line.role, Role::User) && claude_format::is_interrupt_marker(trimmed);
        let is_task_notification =
            matches!(line.role, Role::User) && claude_format::is_task_notification(trimmed);
        let is_human_turn = matches!(line.role, Role::User)
            && !trimmed.is_empty()
            && !is_interrupt_marker
            && !is_task_notification;

        if is_interrupt_marker {
            effects.push(Effect::TurnInterrupted);
        }

        // A synthetic `isApiErrorMessage` assistant line ends the turn on an API
        // error without a `Stop` hook or an interrupt marker. Emit a turn-end
        // effect so the caller feeds the turn machine back to idle; the line is
        // otherwise ingested and attributed like any assistant line (it inherits
        // `carry_thread` via the non-human-turn branch below, so this does not
        // change thread attribution).
        if line.is_api_error {
            effects.push(Effect::TurnAborted);
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
                    // A LEGACY `queued_command` attachment with no matching
                    // send is a programmatic injection, not stray pane typing,
                    // so it must not tear the active turn back to `main` —
                    // inherit the current thread the way a non-human line does.
                    // (Harness-injected `<task-notification>` lines, which
                    // current claude delivers as plain user lines rather than
                    // `queued_command` attachments, are handled earlier: they
                    // are excluded from `is_human_turn` and inherit
                    // `carry_thread` via the `else` branch, like interrupt
                    // markers and tool_result lines.)
                    (state.carry_thread, None)
                }
                None => {
                    state.carry_thread = main_thread;
                    (main_thread, None)
                }
            }
        } else if is_task_notification {
            // A background task's completion: attribute it to the thread that
            // launched the task, not the thread that happens to be current now.
            // The `<tool-use-id>` correlates back to the recorded launch; a
            // match consumes it (emitting `SubagentCompleted` so the persisted
            // correlation is cleared). When the id is unknown — the launch fell
            // in an earlier window no longer seeded into `launched_threads` —
            // fall back to inheriting `carry_thread`, the prior no-regression
            // behaviour.
            let launching_thread = claude_format::task_notification_tool_use_id(trimmed)
                .and_then(|tool_use_id| {
                    state
                        .launched_threads
                        .remove(tool_use_id)
                        .map(|thread| (tool_use_id.to_owned(), thread))
                });
            match launching_thread {
                Some((tool_use_id, thread)) => {
                    effects.push(Effect::SubagentCompleted { tool_use_id });
                    // Advance the turn onto the launching thread: the
                    // assistant's continuation of this notification belongs to
                    // the task's thread, not the thread that was current when
                    // the completion happened to land.
                    state.carry_thread = thread;
                    (thread, None)
                }
                None => (state.carry_thread, None),
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
