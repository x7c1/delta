//! The wire form of [`SessionEvent`].

// Imported here, not in `from_session_event`, so the docs below can link it.
use delta_usecase::SessionEvent;
use serde::Serialize;
use ts_rs::TS;

use crate::file_change_detail::WireFileChangeDetail;
use crate::status_snapshot::WireStatusSnapshot;

mod event_kinds;
pub use event_kinds::event_kinds;
mod from_session_event;
#[cfg(test)]
mod tests;

/// JSON shape of a session event on the `/ws` stream.
///
/// Mirrors the domain [`SessionEvent`] variant-for-variant; see that type for
/// the semantics of each event. This wire twin carries the serialization
/// concerns the domain type must not know about: the `kind` tag, the
/// snake_case variant names, and the TypeScript export. Ids are plain
/// `String`/`i64` here because that is exactly what crosses the wire.
///
/// Only `PartialEq` (not `Eq`) is derived: [`WireSessionEvent::StatusUpdated`]
/// carries a [`WireStatusSnapshot`] with `f64` fields, and `f64` does not
/// implement `Eq`. Nothing keys these events by hash, so `PartialEq` (which
/// backs `assert_eq!`) is all the equality they need.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(rename = "SessionEvent")]
pub enum WireSessionEvent {
    /// The session was registered (first `UserPromptSubmit`).
    SessionRegistered { session_id: String },
    /// A known, previously-closed session became live again (resumed).
    SessionOpened { session_id: String },
    /// An open session was closed: its pane was torn down but its data remains.
    SessionClosed { session_id: String },
    /// A closed session was removed: its row and everything hanging off it are
    /// gone, so a client must drop the session rather than list it as closed.
    /// Only Delta's rows go — the worktree and the agent's own files survive.
    SessionRemoved { session_id: String },
    /// A held (`queued`) send was promoted to `dispatched` and typed.
    SendDispatched { session_id: String, send_id: i64 },
    /// A dispatched send was returned to the queue, held for an explicit
    /// release, after nothing was heard about it twice running: the row is
    /// `queued` with `held_at` set (so it stays in the open-send list with
    /// Send and Cancel actions, and never auto-dispatches) instead of being
    /// re-typed forever. The event is what explains *why* that row is waiting;
    /// `text` repeats the composed message so a client can name it without
    /// refetching the open-send list, which is where the message itself now
    /// lives.
    SendParked {
        session_id: String,
        send_id: i64,
        text: String,
    },
    /// A turn started: sent for every prompt submission, a Delta send or not.
    /// `thread_id` is the thread that took the turn, so the running indicator
    /// lights on the exact thread rather than the whole session. `send_id` is
    /// the send the prompt consumed and `matched_uuid` that send's user line;
    /// each is `null` when not known (a prompt typed into the pane or a
    /// background-task notification has no send; a send's line may not be in
    /// the transcript yet).
    TurnStarted {
        session_id: String,
        send_id: Option<i64>,
        thread_id: i64,
        matched_uuid: Option<String>,
    },
    /// External input was detected (typed directly into the pane).
    ExternalInput { session_id: String, prompt: String },
    /// A response completed. `thread_id` is the thread whose in-flight turn just
    /// ended, so the running indicator clears (and an unread badge bumps when
    /// the thread is not focused) on the exact thread. `null` only for a `Stop`
    /// on a session that was never registered (no thread to resolve).
    TurnCompleted {
        session_id: String,
        thread_id: Option<i64>,
        stop_reason: Option<String>,
    },
    /// The in-flight turn was interrupted by the user (Escape / Ctrl-C).
    /// `thread_id` is the interrupted turn's thread, so its running indicator
    /// clears on the exact thread. `null` only when no thread is resolvable.
    TurnInterrupted {
        session_id: String,
        thread_id: Option<i64>,
    },
    /// The transcript grew between hooks (continuous tail).
    TranscriptUpdated {
        session_id: String,
        thread_ids: Vec<i64>,
    },
    /// A tool permission prompt is imminent.
    PermissionRequested {
        session_id: String,
        request_id: i64,
        tool_name: String,
        /// The tool input, serialized as JSON text, so the notice can show
        /// what the tool is about to do next to its Allow/Deny buttons.
        tool_input: String,
        /// What allowing the request would do to files on disk, when the
        /// provider stated it before asking — so the card can name the files
        /// and show the diff instead of a truncated blob of request params.
        ///
        /// Absent from the frame entirely when nothing is known, which is every
        /// request that is not a file change: the card then renders from
        /// `tool_input` alone, byte-for-byte the frame it has always been.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        file_change: Option<WireFileChangeDetail>,
        /// A directory the request also asks to be allowed to write under for
        /// the rest of the session, when the provider asked for one.
        ///
        /// A **broader** ask than `file_change`, and independent of it: that
        /// names the files this one request would touch, while this is a
        /// standing permission over a whole tree. It is present or absent on its
        /// own — including on a request that carries no `file_change` at all —
        /// so a client must render it separately rather than as another path in
        /// the change list.
        ///
        /// Absent from the frame entirely when the provider asked for no root,
        /// which is every request that is not a file change.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        grant_root: Option<String>,
    },
    /// Claude Code's `AskUserQuestion` tool is presenting a multiple-choice
    /// question; the user picks an option in the TUI.
    QuestionAsked {
        session_id: String,
        request_id: i64,
        /// The in-flight turn's thread, so the browser only shows the question
        /// card on the thread it belongs to.
        thread_id: i64,
        /// The raw `{"questions":[…]}` tool input, serialized as JSON text, so
        /// the browser can render the question card.
        tool_input: String,
    },
    /// A previously-requested tool permission was resolved.
    PermissionResolved { session_id: String, request_id: i64 },
    /// A freshly-spawned session's pane is up but nothing has bound it yet: the
    /// launch was prepared, the tmux session exists and the agent is running in
    /// it, and the embedded terminal can be attached to it from here. Pane-backed
    /// (Claude) launches only — a Codex launch has no pane and binds as its own
    /// last step.
    SpawnPaneReady {
        /// The Delta-minted session id whose pane came up.
        session_id: String,
        /// The tmux session backing it — the same name a later `spawn_failed`
        /// for this launch would carry.
        pane_token: String,
    },
    /// A freshly-spawned session failed to come up before it ever registered.
    /// Emitted for every provider: an adapter-backed (Codex) launch is
    /// accepted and prepared in the background exactly like a Claude one.
    SpawnFailed {
        /// The Delta-minted session id — the only key a client correlates the
        /// failure by.
        session_id: String,
        /// The tmux session that was torn down, for a pane-backed (Claude)
        /// launch.
        ///
        /// Absent from the frame entirely for an adapter-backed launch, which
        /// never had a pane to name.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        pane_token: Option<String>,
        /// Why the launch failed, when Delta can name the cause — the
        /// background launch preparation's own error (a git, tmux or adapter
        /// message), or, for a spawn the watchdog reaped, the deadline it
        /// missed plus what its pane was still showing.
        ///
        /// Absent from the frame entirely where not even that is available: a
        /// launch that exited, and a resume that never became ready. A client
        /// renders it under its "failed to start" text when present and shows
        /// that text alone otherwise.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        reason: Option<String>,
        /// Whether the user asked for this: `true` only for the explicit close
        /// of a still-starting session (`POST /api/sessions/{id}/close`, which
        /// cancels its launch), `false` for the three producers that report a
        /// launch which broke on its own.
        ///
        /// Always present, so a client reads it without a presence check. It
        /// is the stable key for telling a requested cancel from a breakage —
        /// `reason` is prose to display, not something to match on — so a
        /// client that words the two differently branches here.
        cancelled: bool,
    },
    /// A chunk of the in-flight turn's assistant message, streamed live.
    AssistantStreaming {
        session_id: String,
        thread_id: i64,
        message_id: String,
        index: u32,
        /// `final` is a Rust keyword, so the field is `is_final` here while the
        /// wire key stays `final` (the client accumulates until it is `true`).
        #[serde(rename = "final")]
        is_final: bool,
        delta: String,
    },
    /// A subagent started running (its own transcript is never tailed, so this
    /// is the only live signal): an `Agent`/`Task` tool call inside a turn, or
    /// the background agent Claude Code forks for a slash command's skill, which
    /// arrives with NO turn in flight at all — so a client must not scope an
    /// entry to the current turn. Correlated to its `subagent_finished` by
    /// `tool_use_id`.
    SubagentStarted {
        session_id: String,
        /// The thread that launched the subagent, so the client keeps that
        /// thread's running indicator lit — and its unread badge suppressed —
        /// until the subagent finishes, which for a background subagent outlives
        /// the launching turn. `subagent_finished` carries no thread; the client
        /// maps its `tool_use_id` back to this entry's thread.
        thread_id: i64,
        tool_use_id: String,
        subagent_type: Option<String>,
        description: Option<String>,
        /// Whether the launch runs in the background — `run_in_background: true`
        /// for a tool call, and always true for a forked skill. A background
        /// subagent outlives the launching turn (the client must not sweep it at
        /// turn end) and is finished by its completion notification.
        background: bool,
    },
    /// A subagent finished running, correlated to its `subagent_started` by
    /// `tool_use_id`.
    SubagentFinished {
        session_id: String,
        tool_use_id: String,
    },
    /// The latest usage snapshot for a session: selected model, context-window
    /// usage, the account's rate limits, and cost. A "latest value" keyed by
    /// `session_id` (each snapshot supersedes the last), not an append.
    StatusUpdated {
        session_id: String,
        snapshot: WireStatusSnapshot,
    },
    /// An asynchronous repository clone (`POST /api/repositories/clone`)
    /// finished: the working tree now exists at `destination_path`.
    ///
    /// The one event family on this stream carrying **no `session_id`** —
    /// cloning is a workspace-level command with no session behind it. Clients
    /// key it by `repo_owner`/`repo_name` and refetch the PR and repository
    /// lists, whose `has_local_clone` / clone rows it flips.
    RepositoryCloneCompleted {
        repo_owner: String,
        repo_name: String,
        clone_root: String,
        /// `<clone_root>/<repo_name>`. The clone is renamed onto this path
        /// atomically, so its existence means a finished clone, never a partial
        /// one.
        destination_path: String,
    },
    /// An asynchronous repository clone failed. `destination_path` does not
    /// exist when this arrives (the clone is assembled in a temporary sibling
    /// directory, removed on failure), so retrying is the same request again.
    RepositoryCloneFailed {
        repo_owner: String,
        repo_name: String,
        clone_root: String,
        destination_path: String,
        /// Why it failed, as `gh` reported it — shown to the user verbatim.
        message: String,
    },
}
