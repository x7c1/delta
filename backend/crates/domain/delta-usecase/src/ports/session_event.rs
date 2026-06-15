//! An event the Interactor emits for the browser to render.

use delta_model::{MessageUuid, SessionId, ThreadId};

/// An event the Interactor emits for the browser to render.
///
/// This is the domain view of the event: it carries no serialization
/// concerns. The JSON put on the WebSocket is defined by its wire twin
/// (`WireSessionEvent` in the `delta-wire` crate), which mirrors these
/// variants and owns the `kind`-tagged shape plus the generated TypeScript
/// bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// A queued send was confirmed as a turn start.
    TurnStarted {
        session_id: SessionId,
        send_id: i64,
        matched_uuid: MessageUuid,
    },
    /// External input was detected (typed directly into the pane).
    ExternalInput {
        session_id: SessionId,
        prompt: String,
    },
    /// A response completed.
    TurnCompleted {
        session_id: SessionId,
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
    TurnInterrupted { session_id: SessionId },
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
    /// This is the FOREGROUND (synchronous) case: the matching
    /// `PostToolUse(Agent)` fires when the subagent completes. Background
    /// subagents (`run_in_background: true`) complete via a different signal and
    /// are out of scope here.
    SubagentStarted {
        session_id: SessionId,
        /// The `tool_use_id` of the `Agent`/`Task` call (the correlation key).
        tool_use_id: String,
        /// The subagent type (e.g. `general-purpose`), if the call carried one.
        subagent_type: Option<String>,
        /// The short task description, if the call carried one, for display.
        description: Option<String>,
    },
    /// A subagent (the `Agent`/`Task` tool) finished running.
    ///
    /// Detected from the main session's `PostToolUse` hook with `tool_name` in
    /// `{Agent, Task}`, correlated to its [`Self::SubagentStarted`] by
    /// `tool_use_id`. A `PostToolUse` for an id that was never tracked (or was
    /// already cleared at turn end) is a no-op and emits nothing.
    SubagentFinished {
        session_id: SessionId,
        tool_use_id: String,
    },
}
