//! The attribution fold: parsed transcript lines in, attributed messages and
//! effects out.

use std::collections::{BTreeMap, HashSet, VecDeque};

use delta_model::{ContentBlock, Message, MessageUuid, PromptId, Role, Send, SessionId, ThreadId};

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
    /// The background-task identifier learned for the matching subagent launch,
    /// when one has been observed. Unused for human-prompt echo matching (which
    /// is text-based), present so a single struct can also carry the task-id
    /// correlation used to finish a background subagent when its
    /// `<task-notification>` is dropping the `<tool-use-id>` element.
    pub task_id: Option<String>,
}

impl From<&Send> for OutstandingSend {
    fn from(send: &Send) -> Self {
        Self {
            id: send.id,
            thread_id: send.thread_id,
            semantic_parent_uuid: send.semantic_parent_uuid.clone(),
            text: send.text.clone(),
            // The `Send` row has no background-task identifier of its own;
            // task ids are minted per subagent launch and learned later via the
            // `PostToolUse(Agent)` hook (see `RunningSubagent::task_id`).
            task_id: None,
        }
    }
}

/// One outstanding background-task launch: the launching thread of the task,
/// plus the [`task_id`] learned from the launching tool's `tool_result` once
/// the `PostToolUse(Agent)` hook ran. The map [`AttributionState::launched_threads`]
/// keys these by the launching tool_use id.
///
/// [`task_id`]: SubagentLaunch::task_id
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentLaunch {
    /// The thread the launching `Agent`/`Task`/`Bash` tool_use was attributed
    /// to. A completion `<task-notification>` carrying this launch's id (or
    /// matching `task_id`) is attributed back to this thread.
    pub thread_id: ThreadId,
    /// The background-task identifier the launching tool's `tool_result`
    /// reported, learned via the `PostToolUse(Agent)` hook. `None` until that
    /// hook has run, or when the upgrade was never persisted (an older row).
    /// Recorded so that a `<task-notification>` whose `<tool-use-id>` element
    /// was stripped can still be matched by its `<task-id>` element.
    pub task_id: Option<String>,
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
    /// The outstanding background task launches, keyed by the launching
    /// tool_use `id` (the `toolu_...` value). A background subagent or
    /// background Bash (`run_in_background: true`) returns immediately and its
    /// completion is injected later as a `<task-notification>` user line
    /// carrying that same id in its `<tool-use-id>` element. Looking the id up
    /// here attributes the notification (and the assistant continuation it
    /// drives) to the thread that LAUNCHED the task, instead of blindly
    /// inheriting whatever thread is current when it lands — which is wrong
    /// whenever the user moved threads while the task ran.
    ///
    /// Each entry also carries a `task_id` learned later (via the
    /// `PostToolUse(Agent)` hook reading `agentId` out of the launching tool's
    /// `tool_result`), used as a fallback correlation key when Claude Code's
    /// notification body drops the `<tool-use-id>` element — only the
    /// `<task-id>` survives in that case, and matching by it still routes the
    /// completion to the launching thread.
    ///
    /// A map (not a single head) because several background tasks — launched
    /// from different threads, possibly nested — can be outstanding at once,
    /// and a completion must find its own launch by id. Like `outstanding`,
    /// this survives across sync windows by being seeded from a persisted
    /// store at batch start and mutated through effects: a launch is recorded
    /// ([`Effect::SubagentLaunched`]) when first seen and cleared
    /// ([`Effect::SubagentCompleted`]) when its notification is folded.
    /// `BTreeMap` keeps the seed-from-store ↔ fold round-trip deterministic.
    pub launched_threads: BTreeMap<String, SubagentLaunch>,
    /// The `promptId`s of the slash/local-command groups seen in this fold. A
    /// local command (e.g. `/review-pr`) is recorded as several `type: "user"`
    /// lines sharing one `promptId`: a leading `<local-command-caveat>` (the
    /// only one Claude flags `isMeta`), the bare command-name line, then the
    /// command's `<local-command-stdout>`/`<local-command-stderr>` output.
    /// Recording the caveat's `promptId` here lets the later same-`promptId`
    /// lines be recognized as command machinery (folded to [`Role::Meta`]) and
    /// the command-name line, when it equals an outstanding send, be resolved as
    /// a degenerate completed turn — a local command fires no `UserPromptSubmit`
    /// echo and no `Stop`, so without this its dispatched send would wedge the
    /// turn machine in `AwaitingEcho` forever.
    ///
    /// Threaded through the fold state (like `launched_threads`) so a batch cut
    /// between the caveat and its trailing lines still groups them. It is NOT
    /// seeded from a persisted store: Claude writes a local-command group as one
    /// atomic transcript append (the lines share a timestamp), so the whole
    /// group always lands in a single tail batch in production; whole-history
    /// replay sees the caveat before its members within the one pass. A
    /// `HashSet` is fine: it is only ever membership-tested (never iterated for
    /// output), and its `PartialEq` is order-independent, so the threaded-state
    /// equality the batch-split replay property pins still holds.
    pub local_command_prompts: HashSet<PromptId>,
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
            local_command_prompts: HashSet::new(),
        }
    }

    /// Seed a batch with the outstanding background-launch map alongside the
    /// carry thread and outstanding send. The map carries `(tool_use_id ->
    /// SubagentLaunch)` — the launching thread plus the optional `task_id`
    /// learned later — for every background task still awaiting its
    /// `<task-notification>`.
    pub fn with_launches(
        carry_thread: ThreadId,
        outstanding: Option<OutstandingSend>,
        launched_threads: BTreeMap<String, SubagentLaunch>,
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
    /// A dispatched send was consumed by a slash/local command (e.g. the user
    /// ran `/review-pr`), not by a model turn. A local command is handled
    /// entirely client-side: it fires **no** `UserPromptSubmit` echo and **no**
    /// `Stop` hook, yet Delta dispatched it as a send and moved the turn machine
    /// to `AwaitingEcho`. Without a turn-end signal that send stays outstanding
    /// forever — wedging the single-outstanding rule so no later send dispatches.
    /// This effect is emitted alongside the [`Effect::SendMatched`] that
    /// consumes the send (the command-name line equals the send text inside a
    /// recognized local-command `promptId` group): feed the turn machine back to
    /// idle and notify the browser so the stuck send clears, exactly like
    /// [`Effect::TurnAborted`] does for an API-error turn-end.
    LocalCommandTurnEnded,
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
    /// An `Agent`/`Task` tool_use block was seen in the PARENT session's
    /// transcript: light up the running-subagent indicator for it. Emitted
    /// regardless of `run_in_background` — a foreground subagent and a
    /// background one both need the indicator while they run, and they only
    /// differ in how the indicator is cleared (the matching `PostToolUse` for a
    /// foreground entry, the completion `<task-notification>` for a background
    /// one).
    ///
    /// This is the parent-transcript-driven source of truth for the indicator,
    /// replacing the older PreToolUse-driven mechanism. A nested subagent's own
    /// `Agent`/`Task` tool_use is written to the SUBAGENT's JSONL (not the
    /// parent's), so a fold over the parent's transcript naturally excludes
    /// nested launches — and a nested launch never produces a stuck indicator
    /// on the parent.
    SubagentIndicatorStarted {
        tool_use_id: String,
        thread_id: ThreadId,
        subagent_type: Option<String>,
        description: Option<String>,
        background: bool,
    },
}

/// The outcome of folding one batch of transcript lines.
///
/// Holds `Vec<Message>`, which carries an `f64` (`response_time_ms`), so this
/// derives only `PartialEq` — a float cannot implement `Eq`.
#[derive(Debug, Clone, PartialEq)]
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
///   Recent Claude Code versions sometimes drop `<tool-use-id>` from the
///   notification body while keeping `<task-id>`, so the lookup falls back to
///   the `<task-id>` element matched against each entry's persisted `task_id`
///   (learned at `PostToolUse(Agent)` time). Only when neither key matches a
///   recorded launch (the launch fell in an earlier, no-longer-seeded window,
///   or both elements were stripped) does it fall back to inheriting
///   `carry_thread`.
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
        let trimmed_content = content_text.as_deref().unwrap_or("").trim();

        // Slash/local-command grouping. A local command (e.g. `/review-pr`) is
        // recorded as several `type: "user"` lines sharing one `promptId`: a
        // leading `<local-command-caveat>` Claude flags `isMeta` (already
        // `Role::Meta`), the bare command-name line, then the command's
        // `<local-command-stdout>`/`<local-command-stderr>` output (folded to
        // `Role::Meta` by the parser's content check). Record the caveat's
        // `promptId` so the OTHER members are recognized as command machinery.
        if matches!(line.role, Role::Meta)
            && claude_format::is_local_command_caveat(trimmed_content)
        {
            if let Some(prompt_id) = line.prompt_id.clone() {
                state.local_command_prompts.insert(prompt_id);
            }
        }

        // A `type: "user"` line sharing a recognized local-command `promptId`
        // (the bare command-name line — the output lines already arrive as
        // `Role::Meta`) is command machinery, not a human turn. Fold it to
        // `Role::Meta` so it renders collapsed instead of as a user bubble, and
        // — crucially — exclude it from `is_human_turn` so it does not run
        // through external-input handling on `main`.
        let in_local_command_group = line
            .prompt_id
            .as_ref()
            .is_some_and(|id| state.local_command_prompts.contains(id));
        let role = if in_local_command_group && matches!(line.role, Role::User) {
            Role::Meta
        } else {
            line.role
        };
        let is_local_command_name_line = in_local_command_group
            && matches!(line.role, Role::User)
            && !trimmed_content.is_empty();

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
                    // Re-folding a launch line refreshes the launching thread.
                    // If the same id was already seeded from the persisted
                    // store with a `task_id` upgrade, preserve it — it was
                    // learned later via `PostToolUse(Agent)` and a fresh fold
                    // of the launch line itself has no newer information.
                    // Mirrors the SQL `record_subagent_launch` UPSERT, which
                    // only touches the `thread_id` column.
                    let task_id = state
                        .launched_threads
                        .get(id)
                        .and_then(|launch| launch.task_id.clone());
                    state.launched_threads.insert(
                        id.clone(),
                        SubagentLaunch {
                            thread_id: state.carry_thread,
                            task_id,
                        },
                    );
                    effects.push(Effect::SubagentLaunched {
                        tool_use_id: id.clone(),
                        thread_id: state.carry_thread,
                    });
                }
                _ => {}
            }

            // The running-subagent indicator is driven from this parent-side
            // transcript ingest — NOT from the `PreToolUse` hook. Every
            // `Agent`/`Task` tool_use written to the parent's JSONL lights the
            // indicator (foreground OR background), and is cleared later by the
            // matching `PostToolUse` (foreground) or `<task-notification>`
            // (background). A NESTED subagent's `Agent`/`Task` tool_use is
            // written to the SUBAGENT's JSONL, never the parent's, so this
            // branch is the natural filter: nested launches never produce a
            // parent indicator and can never get stuck.
            if let ContentBlock::ToolUse { id, name, input } = block {
                if claude_format::is_subagent_tool(name) {
                    let subagent_type = claude_format::tool_input_string_field(input, "subagent_type")
                        .map(str::to_owned);
                    let description =
                        claude_format::tool_input_string_field(input, "description").map(str::to_owned);
                    let background = claude_format::launches_in_background(input);
                    effects.push(Effect::SubagentIndicatorStarted {
                        tool_use_id: id.clone(),
                        thread_id: state.carry_thread,
                        subagent_type,
                        description,
                        background,
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
        // Classify against the reclassified `role`: a local-command member that
        // was folded to `Role::Meta` above is no longer a human turn (the
        // command-name line is handled by its own branch below).
        let trimmed = trimmed_content;
        let is_interrupt_marker =
            matches!(role, Role::User) && claude_format::is_interrupt_marker(trimmed);
        let is_task_notification =
            matches!(role, Role::User) && claude_format::is_task_notification(trimmed);
        let is_human_turn = matches!(role, Role::User)
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
        let (thread_id, semantic_parent_uuid) = if is_local_command_name_line {
            // The bare command-name line of a local-command group (e.g.
            // `/review-pr`). Delta dispatched it as a send and the turn machine
            // is `AwaitingEcho`, but a local command fires no `UserPromptSubmit`
            // echo and no `Stop` — so left alone the send wedges the queue
            // forever. When this line's text equals the head outstanding send,
            // treat it as a degenerate completed turn: consume the send
            // (`SendMatched`) and end the turn (`LocalCommandTurnEnded`, which the
            // caller feeds into the turn machine as a `Stop`). The line is
            // command machinery, so it inherits `carry_thread` and never resets
            // to `main`. (If it does NOT match an outstanding send — e.g. a
            // local command typed straight into the pane, never dispatched by
            // Delta — there is nothing to resolve; it simply folds as `Meta`.)
            let head_matches = state
                .outstanding
                .front()
                .is_some_and(|send| send.text.trim() == trimmed);
            if let Some(pending) = head_matches.then(|| state.outstanding.pop_front()).flatten() {
                effects.push(Effect::SendMatched {
                    send_id: pending.id,
                    matched_uuid: line.uuid.clone(),
                });
                effects.push(Effect::LocalCommandTurnEnded);
            }
            (state.carry_thread, None)
        } else if is_human_turn {
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
            // The notification carries two correlation keys — `<tool-use-id>`
            // and `<task-id>` — and Claude Code's user-message body sometimes
            // ships only one of them. Prefer `<tool-use-id>` (the existing key,
            // recorded at launch time); fall back to `<task-id>` (recorded
            // later via `PostToolUse(Agent)`). A match consumes the entry and
            // emits `SubagentCompleted` so the persisted correlation is
            // cleared. When neither key matches a recorded launch — the launch
            // fell in an earlier window no longer seeded into
            // `launched_threads`, or both elements were stripped from the body
            // — fall back to inheriting `carry_thread`, the prior no-regression
            // behaviour. A body carrying NEITHER element is logged so a future
            // Claude Code format change surfaces in the logs instead of as
            // stuck running indicators.
            let notification_tool_use_id = claude_format::task_notification_tool_use_id(trimmed);
            let notification_task_id = claude_format::task_notification_task_id(trimmed);
            if notification_tool_use_id.is_none() && notification_task_id.is_none() {
                tracing::warn!(
                    session_id = %session_id.as_str(),
                    thread_id = state.carry_thread.value(),
                    "<task-notification> body carries no <tool-use-id> nor <task-id>; \
                     cannot match against any launched subagent — the running indicator \
                     will not clear from this notification"
                );
            }
            let by_tool_use_id = notification_tool_use_id
                .filter(|id| state.launched_threads.contains_key(*id))
                .map(str::to_owned);
            let resolved = by_tool_use_id.or_else(|| {
                let task_id = notification_task_id?;
                state
                    .launched_threads
                    .iter()
                    .find(|(_, launch)| launch.task_id.as_deref() == Some(task_id))
                    .map(|(tool_use_id, _)| tool_use_id.clone())
            });
            match resolved.and_then(|key| {
                state
                    .launched_threads
                    .remove(&key)
                    .map(|launch| (key, launch))
            }) {
                Some((tool_use_id, launch)) => {
                    effects.push(Effect::SubagentCompleted { tool_use_id });
                    // Advance the turn onto the launching thread: the
                    // assistant's continuation of this notification belongs to
                    // the task's thread, not the thread that was current when
                    // the completion happened to land.
                    state.carry_thread = launch.thread_id;
                    (launch.thread_id, None)
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
            // The reclassified role: a local-command command-name line folds to
            // `Role::Meta` so it renders collapsed, not as a user bubble.
            role,
            linear_parent_uuid: line.linear_parent_uuid,
            semantic_parent_uuid,
            prompt_id: line.prompt_id,
            // Persist the message's own transcript line index as its `seq`,
            // so ordering follows true file position with no drift.
            seq: line.seq,
            content_text,
            content: line.content,
            created_at: line.created_at,
            // Transcript-derived per-message metadata, carried straight through.
            model: line.model,
            git_branch: line.git_branch,
            cwd: line.cwd,
            response_time_ms: line.response_time_ms,
        });
    }

    Attributed {
        messages,
        effects,
        state,
    }
}
