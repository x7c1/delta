//! An event the Interactor emits for the browser to render.

use delta_model::{AgentProvider, MessageUuid, SessionId, ThreadId};

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
    ///
    /// Pane-backed sessions only: an adapter-backed session (Codex) dispatches
    /// each send as it arrives, so it has no queued→dispatched transition to
    /// announce (see `docs/guides/api/sends.md` for the two dispatch paths).
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
    ///
    /// Pane-backed sessions only: parking is the echo-correlation path's
    /// failure mode, and an adapter-backed session matches on the turn id its
    /// provider returns instead.
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
    ///
    /// Pane-backed sessions only, too: an adapter-backed session has no prompt
    /// hook to fire.
    TurnStarted {
        session_id: SessionId,
        send_id: i64,
        thread_id: ThreadId,
        matched_uuid: MessageUuid,
    },
    /// A prompt matched no outstanding send. Usually the user typed straight
    /// into the pane, but a dispatched send whose echo came back mangled also
    /// lands here.
    ///
    /// Pane-backed sessions only: an adapter-backed session has no pane to
    /// type into and no echo to mismatch.
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
    /// A subagent started running: an `Agent`/`Task` tool call the model made
    /// inside a turn (the historical and current names for the same tool), or
    /// the background agent Claude Code forks for a slash command's skill,
    /// which arrives with NO turn in flight at all — so a consumer must not
    /// scope the entry to the current turn.
    ///
    /// A subagent runs in its own transcript that Delta never tails, so the
    /// main conversation pane shows nothing while it works — this event is the
    /// only live signal that one is running. Both kinds are detected by folding
    /// the PARENT session's transcript — an `Agent`/`Task` `tool_use` block, or
    /// a `<forked-skill-launch>` element on a `local_command` line — not from a
    /// hook: a nested subagent's `tool_use` is written to the subagent's own
    /// JSONL, so it can never light the parent's indicator, and a forked skill
    /// fires no hook for its launch at all. `PreToolUse(Agent)` only forces an
    /// immediate sync so the indicator lights without waiting for the ambient
    /// tail. Correlated to its [`Self::SubagentFinished`] by `tool_use_id`.
    ///
    /// The [`Self::SubagentStarted::background`] flag distinguishes the two
    /// lifecycles. A FOREGROUND (synchronous) subagent finishes when its
    /// matching `PostToolUse(Agent)` fires. A BACKGROUND subagent
    /// (`run_in_background: true`, and always so for a forked skill) returns
    /// immediately at launch — its `PostToolUse` does NOT finish it, and a
    /// forked skill fires none — and finishes only when its completion
    /// `<task-notification>` is folded during transcript sync.
    SubagentStarted {
        session_id: SessionId,
        /// The thread that launched the subagent: the thread the fold attributed
        /// the launch line to — the launching turn's thread for a tool call, and
        /// for a forked skill the thread its local command was attributed to
        /// (that command is already a finished turn). A BACKGROUND subagent
        /// outlives its launching turn, so the browser needs the thread to keep
        /// that thread's running indicator lit (and its unread badge suppressed)
        /// until the subagent finishes — not just for the duration of the turn.
        /// The matching [`Self::SubagentFinished`] carries no thread; the
        /// browser maps the `tool_use_id` back to this entry's thread.
        thread_id: ThreadId,
        /// The launch's correlation key: the `tool_use_id` of the `Agent`/`Task`
        /// call, or the synthetic `forked-skill:<agentId>` minted for a forked
        /// skill, which makes no tool call.
        tool_use_id: String,
        /// The subagent type (e.g. `general-purpose`), a forked skill's skill
        /// name, or `None` if the launch carried neither.
        subagent_type: Option<String>,
        /// The short task description, if the launch carried one, for display.
        description: Option<String>,
        /// Whether the launch runs in the background — `run_in_background: true`
        /// for a tool call, and always true for a forked skill. A background
        /// subagent outlives the launching turn and is finished by its
        /// completion notification, not by its immediate `PostToolUse`.
        background: bool,
    },
    /// A subagent finished running.
    ///
    /// For a FOREGROUND subagent, detected from the main session's `PostToolUse`
    /// hook with `tool_name` in `{Agent, Task}`. For a BACKGROUND subagent — a
    /// `run_in_background: true` tool call, or a forked skill — it is emitted
    /// when the completion `<task-notification>` is folded during transcript
    /// sync (`Effect::SubagentCompleted`), or by the process-gone sweep if the
    /// session ends before that notification can be folded (see
    /// `sweep_running_subagents`). Either way it is correlated to its
    /// [`Self::SubagentStarted`] by `tool_use_id`; a finish for an id that was
    /// never tracked (or was already cleared at turn end) is a no-op and emits
    /// nothing.
    SubagentFinished {
        session_id: SessionId,
        tool_use_id: String,
    },
    /// The latest usage snapshot observed for a session: selected model,
    /// context-window occupancy, the account's rate-limit windows, and cost.
    ///
    /// Provider-neutral, and produced by whichever edge the provider exposes:
    /// Claude's `statusLine` command (injected into the session settings, which
    /// Claude Code invokes on every refresh and which `curl`s the JSON back to
    /// the server), and Codex's pushed `thread/tokenUsage/updated` /
    /// `account/rateLimits/updated` notifications, translated by its adapter.
    /// None of this data is in a Claude transcript, and none of it is persisted
    /// for either provider, so this event is the only way the browser learns it.
    ///
    /// Because these refresh frequently, this is a "latest value" keyed by
    /// `session_id`, not an append: each snapshot supersedes the last. It
    /// carries no turn or thread semantics and mutates no server state.
    ///
    /// A snapshot need not be complete: a provider that reports token usage and
    /// account limits on separate frames emits one event per frame, each
    /// carrying only what that frame said (see [`StatusSnapshot::rate_limits`]
    /// for how "said nothing" is distinguished from "said there are none").
    StatusUpdated {
        session_id: SessionId,
        snapshot: StatusSnapshot,
    },
    /// An asynchronous repository clone finished successfully: the clone the
    /// browser asked for with `POST /api/repositories/clone` now exists at
    /// `destination_path`.
    ///
    /// **Not session-scoped** — the only event family here that is not. Cloning
    /// a repository is a workspace-level command with no session behind it, and
    /// the browser still needs the answer because the job outlives the `202` its
    /// request got. A client keys it by the repository the request named
    /// (`repo_owner`/`repo_name`) and refetches the PR list and the repository
    /// list, whose `has_local_clone` / clone rows this flips.
    ///
    /// Fire-and-forget like every event here, and the job registry behind it is
    /// in-memory only: a client that misses this learns nothing about the clone
    /// until it refetches, and a server restart forgets the job outright.
    RepositoryCloneCompleted {
        repo_owner: String,
        repo_name: String,
        /// The registered clone root the destination sits in — echoed back so a
        /// client that offered a choice of roots can tell which one it went to.
        clone_root: String,
        /// `<clone_root>/<repo_name>`: the finished working tree. The clone is
        /// renamed onto this path atomically, so this path existing means the
        /// clone is complete, never half-written.
        destination_path: String,
    },
    /// An asynchronous repository clone failed. Same shape and delivery
    /// semantics as [`Self::RepositoryCloneCompleted`], plus the reason.
    ///
    /// `destination_path` does NOT exist when this arrives — the clone is
    /// assembled in a temporary sibling directory that is removed on failure —
    /// so a retry is simply the same request again.
    RepositoryCloneFailed {
        repo_owner: String,
        repo_name: String,
        clone_root: String,
        destination_path: String,
        /// Why the clone failed, as the `gh` invocation reported it. Shown to
        /// the user verbatim: "no such repository" and "no network" call for
        /// different reactions, and only the message distinguishes them.
        message: String,
    },
}

/// A snapshot of a session's usage state, as its provider's own edge reported
/// it.
///
/// Every field is optional, because every provider reports a different subset
/// at a different time: before a Claude session's first API response
/// `current_usage` / `used_percentage` are `null`, while Codex reports token
/// usage and account rate limits on two independent notifications, so a given
/// snapshot may speak to only one of them. See [`Self::rate_limits`] for how
/// "this frame says nothing about rate limits" (`None`) is distinguished from
/// "the account has none" (`Some(vec![])`, which is what a non-Pro/Max Claude
/// account's status line reports — not an absent field).
///
/// **The neutral layer never computes any of these numbers.** Each provider's
/// adapter (or hook) is the authority for its own: Claude Code precomputes
/// `used_percentage` against the correct window size and Delta forwards it
/// verbatim; the Codex adapter derives its percentage from the counts and the
/// `modelContextWindow` the server reports, and omits it when the server
/// reports no window. Recomputing here would mean guessing a window size the
/// core does not know.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSnapshot {
    /// Which provider's account and session these numbers describe.
    ///
    /// Load-bearing rather than decorative: rate limits are scoped to an
    /// account × provider, so the browser keys them by this and can never show
    /// one provider's limits while another provider's session is focused.
    pub provider: AgentProvider,
    /// The selected model's stable id (e.g. `claude-opus-4-...`).
    pub model_id: Option<String>,
    /// The selected model's human-readable name (e.g. `Opus 4.8`).
    pub model_display_name: Option<String>,
    /// Percentage of the context window in use, as computed by the provider's
    /// own edge — never recomputed here. `None` when the provider does not
    /// expose enough to say (e.g. Codex before the server reports a
    /// `modelContextWindow`), which reads as "no bar", never as 0%.
    pub context_used_percentage: Option<f64>,
    /// The context window's total size in tokens.
    pub context_window_size: Option<u64>,
    /// Tokens currently occupying the context window.
    pub context_current_usage: Option<u64>,
    /// Total input tokens sent this session.
    pub total_input_tokens: Option<u64>,
    /// The account's rate-limit windows, most significant first.
    ///
    /// `None` means this snapshot makes **no statement** about rate limits (a
    /// Codex token-usage frame says nothing about them, and must not clear
    /// what an earlier account frame reported). `Some(windows)` replaces the
    /// account's windows wholesale — including `Some(vec![])`, which is how a
    /// provider says "this account has no windows to show".
    pub rate_limits: Option<Vec<RateLimitWindow>>,
    /// Total session cost in USD.
    pub total_cost_usd: Option<f64>,
    /// The session's working directory.
    pub current_dir: Option<String>,
}

impl StatusSnapshot {
    /// An empty snapshot for `provider`: it states nothing at all, so a caller
    /// fills in only the facts its frame actually carried. There is no
    /// `Default`, because a snapshot with no provider would be a snapshot whose
    /// rate limits belong to nobody.
    pub fn new(provider: AgentProvider) -> Self {
        Self {
            provider,
            model_id: None,
            model_display_name: None,
            context_used_percentage: None,
            context_window_size: None,
            context_current_usage: None,
            total_input_tokens: None,
            rate_limits: None,
            total_cost_usd: None,
            current_dir: None,
        }
    }
}

/// One rate-limit window from a [`StatusSnapshot`].
///
/// Windows are identified by their **duration**, not by a name. Claude reports
/// two fixed windows (5 hours and 7 days) and Codex reports anonymous
/// `primary` / `secondary` windows carrying an explicit `windowDurationMins`,
/// so mapping either provider's windows onto the other's names would be a
/// fiction. Carrying the duration as data lets the browser label and pace a
/// window it has never seen before.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RateLimitWindow {
    /// How long the window is, in seconds — its identity, and what the browser
    /// labels the row from (`5h`, `7d`). `None` when the provider reports a
    /// window without saying how long it is; the window is still shown (its
    /// percentage is real), just unlabelled.
    pub duration_seconds: Option<i64>,
    /// Percentage of the window consumed.
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds at which the window resets.
    pub resets_at: Option<i64>,
}
