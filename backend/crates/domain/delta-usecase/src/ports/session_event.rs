//! An event the Interactor emits for the browser to render.

use delta_model::{MessageUuid, SessionId, ThreadId};

/// An event the Interactor emits for the browser to render.
///
/// This is the domain view of the event: it carries no serialization
/// concerns. The JSON put on the WebSocket is defined by its wire twin
/// (`WireSessionEvent` in the `delta-wire` crate), which mirrors these
/// variants and owns the `kind`-tagged shape plus the generated TypeScript
/// bindings.
///
/// Only `PartialEq` (not `Eq`) is derived: [`Self::StatusUpdated`] carries a
/// [`StatusSnapshot`] with `f64` fields (percentages, cost), and `f64` does not
/// implement `Eq`. No code keys events by hash or stores them in a set, so
/// `PartialEq` is all the equality the events ever need (it backs `assert_eq!`
/// in tests).
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    /// The session was registered (first `UserPromptSubmit`).
    ///
    /// This doubles as the "opened" signal for a freshly-spawned session: a new
    /// spawn has no `session_id` until its first hook binds it, so its first
    /// liveness signal is this registration rather than a separate
    /// [`Self::SessionOpened`].
    SessionRegistered { session_id: SessionId },
    /// A known, previously-closed session became live again (resumed).
    ///
    /// Emitted when a session is reopened by id (e.g. `claude --resume`). A
    /// brand-new session never emits this — its first liveness signal is
    /// [`Self::SessionRegistered`]. Focus is purely client-side; this only says
    /// the session now has a live pane.
    SessionOpened { session_id: SessionId },
    /// An open session was closed: its pane was torn down but its data remains.
    SessionClosed { session_id: SessionId },
    /// A held (`queued`) send was promoted to `dispatched` and its keystrokes
    /// typed: the session went idle and the send took its turn.
    ///
    /// Lets the browser refetch the open-send list the moment the
    /// queued→dispatched transition happens, instead of waiting for the next
    /// turn-lifecycle event.
    SendDispatched { session_id: SessionId, send_id: i64 },
    /// A dispatched send was abandoned after its echo failed to match twice.
    ///
    /// Delta correlates a send with the `UserPromptSubmit` it produces by
    /// text; a mismatch returns the send to `queued` to be re-typed on the
    /// next idle. The turn machine allows that retry once and then *parks* the
    /// send: the row is cancelled (so it leaves the open-send list and the
    /// pending chip stops spinning) and this event says so, carrying the
    /// composed `text` back so the browser can show what was not delivered
    /// instead of losing it silently.
    ///
    /// Session-scoped, not thread-scoped: an undelivered message is the user's
    /// problem wherever they happen to be looking.
    ///
    /// Fire-and-forget, like every event here: it is not replayed on reconnect
    /// and there is no queryable field a refetch could re-seed it from (the
    /// parked row is `cancelled`, so it is out of the open-send list). A
    /// browser that was disconnected when the park happened therefore learns
    /// nothing; the text survives only on the cancelled row in the store.
    /// Making it recoverable is a follow-up for the attribution redesign, not
    /// something this seam can do on its own.
    SendParked {
        session_id: SessionId,
        send_id: i64,
        /// The composed message that was never delivered.
        text: String,
    },
    /// A queued send was confirmed as a turn start.
    ///
    /// `thread_id` is the thread the dispatched send was composed for, so the
    /// browser can light the running indicator on the exact thread (main or a
    /// branch) that took the turn rather than the session as a whole.
    TurnStarted {
        session_id: SessionId,
        send_id: i64,
        thread_id: ThreadId,
        matched_uuid: MessageUuid,
    },
    /// External input was detected (typed directly into the pane).
    ExternalInput {
        session_id: SessionId,
        prompt: String,
    },
    /// A response completed.
    ///
    /// `thread_id` is the thread whose in-flight turn just ended, recovered
    /// (via `SessionStore::in_progress_turn_thread`) before the turn machine
    /// clears the in-flight send, so the browser clears the running indicator —
    /// and bumps the unread badge when the thread is not focused — on the exact
    /// thread that ran. `None` only for the degenerate case of a `Stop` on a
    /// session Delta never registered (no thread exists to resolve); a real
    /// turn always carries its thread.
    TurnCompleted {
        session_id: SessionId,
        thread_id: Option<ThreadId>,
        stop_reason: Option<String>,
    },
    /// The in-flight turn was interrupted by the user (Escape / Ctrl-C).
    ///
    /// Detected from the transcript itself rather than a hook: when the user
    /// interrupts, Claude's `Stop` hook does not fire, so [`Self::TurnCompleted`]
    /// is never emitted and the optimistic "open send" chip would stay
    /// "in progress" forever. The transcript tail instead sees a discrete
    /// `[Request interrupted by user...]` user line and emits this event, which
    /// clears the stuck send hook-independently (same delivery path as
    /// [`Self::PermissionResolved`]).
    ///
    /// `thread_id` is the thread whose in-flight turn was interrupted, recovered
    /// the same way [`Self::TurnCompleted`] recovers its thread, so the browser
    /// clears the running indicator on the exact thread that was interrupted.
    /// `None` only when no thread is resolvable (a degenerate signal for a
    /// session with no in-flight turn); a real interrupt always carries it.
    TurnInterrupted {
        session_id: SessionId,
        thread_id: Option<ThreadId>,
    },
    /// The transcript grew between hooks (continuous tail).
    ///
    /// Emitted by the background poll when new lines were ingested, so the
    /// browser refetches the affected threads. Unlike [`Self::TurnCompleted`]
    /// and [`Self::ExternalInput`], this carries no turn semantics and must not
    /// mutate the open-send FIFO or unread badges — it is a pure
    /// "refetch these threads" signal. `thread_ids` are the distinct threads of
    /// the newly-ingested messages.
    TranscriptUpdated {
        session_id: SessionId,
        thread_ids: Vec<ThreadId>,
    },
    /// A tool permission prompt is imminent.
    PermissionRequested {
        session_id: SessionId,
        request_id: i64,
        tool_name: String,
        /// The tool input, serialized as JSON text, so the notice can show
        /// what the tool is about to do (e.g. the command a `Bash` call runs)
        /// next to its Allow/Deny buttons.
        tool_input_json: String,
    },
    /// Claude Code's built-in `AskUserQuestion` tool is presenting a
    /// multiple-choice question, so the user must pick an option in the TUI.
    ///
    /// Driven off the `PreToolUse` hook (which records the request row carrying
    /// the `tool_use_id`), so the same `tool_result` → `PermissionResolved`
    /// path that clears a permission notice also clears this one once the user
    /// answers. `request_id` is that `PreToolUse` row id; `tool_input_json` is
    /// the raw `{"questions":[…]}` payload the browser parses to render the
    /// question card.
    ///
    /// Unlike a permission request, this carries no Allow/Deny: a hook cannot
    /// return the selected answer, so Delta only surfaces the question — the
    /// answer is given in the TUI. The assistant's preamble text is *not*
    /// available here: Claude flushes it to the transcript only after the user
    /// answers.
    ///
    /// `thread_id` is the in-flight turn's thread, so the browser only shows
    /// the question card on the thread it belongs to — recovered the same way
    /// the streaming preview ([`Self::AssistantStreaming`]) recovers its thread.
    QuestionAsked {
        session_id: SessionId,
        request_id: i64,
        thread_id: ThreadId,
        tool_input_json: String,
    },
    /// A previously-requested tool permission was resolved.
    ///
    /// Emitted when the browser decides via
    /// `POST /api/permissions/{id}/decision`, or when the `tool_result`
    /// correlated with an open [`Self::PermissionRequested`] is ingested. An
    /// auto-approved tool resolves almost immediately (the result lands right
    /// away), so the browser clears the notice promptly; a genuine TUI prompt
    /// yields no result until the human answers (there or in the browser), so
    /// the notice persists until then.
    PermissionResolved {
        session_id: SessionId,
        request_id: i64,
    },
    /// A freshly-spawned session failed to come up: its launch ended (or never
    /// got far enough) before it ever registered via its first
    /// `UserPromptSubmit`, so it never bound to a live session.
    ///
    /// A new spawn is fire-and-forget — `claude` is launched in a tmux pane and
    /// the only thing that registers/binds it is the first `UserPromptSubmit`
    /// hook. If `claude` crashes, exits, or hangs on auth before that hook ever
    /// fires, nothing would otherwise time the dangling spawn out and the UI is
    /// stuck "pending" forever with no error. This event is the failure signal:
    /// it is emitted either by the `SessionEnd` hook (the launch exited while
    /// still unbound — the immediate case) or by the watchdog reaper (the spawn
    /// outlived its deadline without ever binding). It carries the Delta-minted
    /// `session_id` so the browser can correlate it to the optimistic pending
    /// chip, plus the `pane_token` of the tmux session that was torn down.
    SpawnFailed {
        session_id: SessionId,
        pane_token: String,
    },
    /// A chunk of the in-flight turn's assistant message, delivered live while
    /// the turn is still generating.
    ///
    /// Claude Code's `MessageDisplay` hook fires repeatedly during generation,
    /// before the transcript JSONL is flushed, so this carries the visible
    /// assistant text as it appears — a provisional live preview of the
    /// in-flight turn's reply. The chunks share one `message_id` and arrive at
    /// increasing `index` (the client accumulates them); only the last has
    /// `final == true`.
    ///
    /// This is NOT persisted: the transcript stays the source of truth. The
    /// hook's ids do not match any transcript id, so the preview cannot be
    /// id-joined to the persisted message; instead it is attributed to the
    /// in-flight turn's thread and dropped per turn — once the turn ends
    /// ([`Self::TurnCompleted`] / [`Self::TurnInterrupted`]) the persisted
    /// assistant message, ingested by the normal transcript sync, takes over.
    AssistantStreaming {
        session_id: SessionId,
        thread_id: ThreadId,
        /// The display message these chunks belong to (the hook's own id; not a
        /// transcript id).
        message_id: String,
        index: u32,
        final_: bool,
        delta: String,
    },
    /// A subagent (the `Agent`/`Task` tool) started running inside the main
    /// turn.
    ///
    /// A subagent runs in its own transcript that Delta never tails, so the
    /// main conversation pane shows nothing while it works — this event is the
    /// only live signal that one is running. Detected from the main session's
    /// `PreToolUse` hook with `tool_name` in `{Agent, Task}` (the historical and
    /// current names for the same tool); the nested tool calls a subagent makes
    /// reach the same hook but never match those names, so they do not flip the
    /// indicator. Correlated to its [`Self::SubagentFinished`] by `tool_use_id`.
    ///
    /// The [`Self::SubagentStarted::background`] flag distinguishes the two
    /// lifecycles. A FOREGROUND (synchronous) subagent finishes when its
    /// matching `PostToolUse(Agent)` fires. A BACKGROUND subagent
    /// (`run_in_background: true`) returns immediately at launch — its
    /// `PostToolUse` does NOT finish it — and finishes only when its completion
    /// `<task-notification>` is folded during transcript sync.
    SubagentStarted {
        session_id: SessionId,
        /// The thread that launched the subagent, resolved (via
        /// `SessionStore::in_progress_turn_thread`) the same way `TurnStarted`
        /// resolves its thread. A BACKGROUND subagent outlives its launching
        /// turn, so the browser needs the thread to keep that thread's running
        /// indicator lit (and its unread badge suppressed) until the subagent
        /// finishes — not just for the duration of the turn. The matching
        /// [`Self::SubagentFinished`] carries no thread; the browser maps the
        /// `tool_use_id` back to this entry's thread.
        thread_id: ThreadId,
        /// The `tool_use_id` of the `Agent`/`Task` call (the correlation key).
        tool_use_id: String,
        /// The subagent type (e.g. `general-purpose`), if the call carried one.
        subagent_type: Option<String>,
        /// The short task description, if the call carried one, for display.
        description: Option<String>,
        /// Whether the launch carried `run_in_background: true`. A background
        /// subagent outlives the launching turn and is finished by its
        /// completion notification, not by its immediate `PostToolUse`.
        background: bool,
    },
    /// A subagent (the `Agent`/`Task` tool) finished running.
    ///
    /// For a FOREGROUND subagent, detected from the main session's `PostToolUse`
    /// hook with `tool_name` in `{Agent, Task}`. For a BACKGROUND subagent, it
    /// is emitted when the completion `<task-notification>` is folded during
    /// transcript sync (`Effect::SubagentCompleted`). Either way it is
    /// correlated to its [`Self::SubagentStarted`] by `tool_use_id`; a finish
    /// for an id that was never tracked (or was already cleared at turn end) is
    /// a no-op and emits nothing.
    SubagentFinished {
        session_id: SessionId,
        tool_use_id: String,
    },
    /// The latest Claude Code status-line snapshot for a session.
    ///
    /// Sourced from the `statusLine` command Delta injects into the session
    /// settings, which Claude Code invokes on every status-line refresh (the
    /// command `curl`s the JSON back to the server). None of this data is in the
    /// transcript JSONL, so this event is the only way the browser learns the
    /// session's selected model, context-window usage, rate limits, and cost.
    ///
    /// Because the status line refreshes frequently, this is a "latest value"
    /// keyed by `session_id`, not an append: each snapshot supersedes the last.
    /// It carries no turn or thread semantics and mutates no server state.
    StatusUpdated {
        session_id: SessionId,
        snapshot: StatusSnapshot,
    },
}

/// A snapshot of Claude Code session state from the `statusLine` command.
///
/// Mirrors the fields Delta extracts from the raw `statusLine` JSON
/// (`delta_wire::hooks::StatusLinePayload`). Every field is optional: before a
/// session's first API response Claude Code reports `current_usage` /
/// `used_percentage` as `null` and omits `rate_limits` entirely (also omitted
/// on accounts without a Pro/Max subscription).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusSnapshot {
    /// The selected model's stable id (e.g. `claude-opus-4-...`).
    pub model_id: Option<String>,
    /// The selected model's human-readable name (e.g. `Opus 4.8`).
    pub model_display_name: Option<String>,
    /// Percentage of the context window in use, as precomputed by Claude Code.
    pub context_used_percentage: Option<f64>,
    /// The context window's total size in tokens.
    pub context_window_size: Option<u64>,
    /// Tokens currently occupying the context window.
    pub context_current_usage: Option<u64>,
    /// Total input tokens sent this session.
    pub total_input_tokens: Option<u64>,
    /// The 5-hour rate-limit window.
    pub five_hour: Option<RateLimitWindow>,
    /// The 7-day rate-limit window.
    pub seven_day: Option<RateLimitWindow>,
    /// Total session cost in USD.
    pub total_cost_usd: Option<f64>,
    /// The session's working directory.
    pub current_dir: Option<String>,
}

/// One rate-limit window from a [`StatusSnapshot`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RateLimitWindow {
    /// Percentage of the window consumed.
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds at which the window resets.
    pub resets_at: Option<i64>,
}
