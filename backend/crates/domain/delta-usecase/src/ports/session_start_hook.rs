//! Payload of a `SessionStart` hook.

use delta_model::SessionId;

/// Payload of a `SessionStart` hook.
///
/// Claude Code fires `SessionStart` once a session's TUI is ready to accept
/// input. It is the readiness signal Delta gates launch on:
///
/// - `source=startup` — a fresh launch reached its prompt. If the id matches a
///   still-pending spawn, Delta binds it now (even a prompt-less plain spawn
///   registers immediately, rather than waiting for the first
///   `UserPromptSubmit`).
/// - `source=resume` — `claude --resume <id>` finished replaying and is ready.
///   Delta releases the held first prompt for that session, dispatching it on
///   the normal `send_line` path now that the cold pane can accept it.
/// - `source=compact` — fires mid-session on an already-live session once
///   Claude Code finishes auto- or manually compacting it. Not a launch (so
///   binding/readiness handling stays out of it), but the compaction routine
///   may have swallowed a prompt the user keyed in at the same moment, so
///   Delta re-types any `Dispatched` `OutstandingSend` for the session —
///   debounced against the ingestion-time `Effect::AutoCompactFinished` path
///   so the live and replay signals do not double-submit.
/// - `source=clear` — fires mid-session on an already-live session when the
///   user deliberately wipes the context. Not a launch and not a recovery
///   point either: outstanding sends are left alone (resurrecting them would
///   invert the wipe's intent), so this stays a safe no-op.
#[derive(Debug, Clone)]
pub struct SessionStartHook {
    /// The Claude `session_id` the session runs under. For a Delta spawn this is
    /// the id Delta pinned via `--session-id`; for a resume it is the id Delta
    /// passed to `--resume`. Either way it is the binding/readiness key.
    pub session_id: SessionId,
    /// Why the session started: `startup`, `resume`, `clear`, or `compact`.
    /// Carried verbatim from the hook; the usecase gates on it to tell a real
    /// launch (startup/resume) apart from a mid-session event (compact, which
    /// triggers the stuck-send re-dispatch; clear, which is a safe no-op).
    pub source: String,
    /// The session's working directory, carried like every hook payload. Used to
    /// register the session row on a `source=startup` bind, so a prompt-less
    /// plain spawn can register from `SessionStart` without waiting for the first
    /// `UserPromptSubmit`.
    pub cwd: String,
    /// The session's JSONL transcript path, carried like every hook payload.
    /// Stored on the session row at `source=startup` registration so later syncs
    /// know where to read.
    pub transcript_path: String,
}

impl SessionStartHook {
    /// `source` value for a fresh launch reaching its prompt.
    pub const SOURCE_STARTUP: &'static str = "startup";
    /// `source` value for a `claude --resume` that finished replaying.
    pub const SOURCE_RESUME: &'static str = "resume";
    /// `source` value for a mid-session auto/manual `/compact`. The session is
    /// already live; the hook fires once Claude Code has finished writing the
    /// compaction group (caveat / command-name / summary / stdout) and is back
    /// at its prompt. Delta uses this signal to re-type any `Dispatched`
    /// `OutstandingSend` whose echo was swallowed by the compaction routine —
    /// without re-dispatch the chip stays "in progress" forever.
    pub const SOURCE_COMPACT: &'static str = "compact";
    /// `source` value for a mid-session `/clear`. The session is already live
    /// but the user deliberately wiped its context; Delta leaves outstanding
    /// sends alone here — resurrecting them would invert intent.
    pub const SOURCE_CLEAR: &'static str = "clear";
}
