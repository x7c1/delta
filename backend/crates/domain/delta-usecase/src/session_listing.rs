//! A stored session annotated with its live (open) state for the session list.

use delta_model::{Session, ThreadId};

/// A stored session plus the runtime facts the browser needs to render it.
///
/// `list_sessions_page` returns these so the navigator can show every
/// conversation Delta knows — whether or not it currently has a live pane — and
/// route into each one. Open/closed is process-runtime state from the registry,
/// not a persisted column, so it is computed per call rather than read off
/// `session`. That same runtime state drives the list's open-first ordering:
/// every live session leads — including a spawn in flight, which still reports
/// `open: false` — then the closed ones, each group most-recently-active first.
#[derive(Debug, Clone)]
pub struct SessionListing {
    /// The persisted session record.
    pub session: Session,
    /// Whether the session currently has a live pane (is resumable into without
    /// a `--resume`). A closed session still appears, just with `open: false`.
    pub open: bool,
    /// Whether the session holds a pane a terminal may attach to that nothing
    /// has bound yet — a launch whose tmux pane is up while its first hook has
    /// not arrived.
    ///
    /// That window is when a launch may be sitting on an interactive prompt
    /// only a human can answer (Claude Code's workspace-trust dialog), so the
    /// browser has to be able to open the terminal on it. Carrying it on the
    /// row rather than only announcing it once (`spawn_pane_ready`) is what
    /// lets a browser that was not listening — a reload mid-launch, a second
    /// tab — still find out.
    ///
    /// Read off the same pane the PTY bridge resolves against, so an attach
    /// while it is `true` succeeds. Mutually exclusive with `open`, which is
    /// what carries the pane once binding ends this window — `false` here is
    /// not "no pane".
    pub pane_starting: bool,
    /// The id of the session's trunk (`main`) thread, for drilling in.
    pub main_thread_id: ThreadId,
    /// The timestamp of the session's most recent message (ISO-8601 UTC), or
    /// `None` when the session has no messages yet. Read from the denormalized
    /// `session.last_activity_at` column (maintained on every message upsert),
    /// so the session list orders within each open/closed group by it without
    /// recomputing recency per row.
    pub last_activity_at: Option<String>,
}
