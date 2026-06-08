//! A stored session annotated with its live (open) state for the session list.

use delta_model::{Session, ThreadId};

/// A stored session plus the runtime facts the browser needs to render it.
///
/// `list_sessions` returns these so the navigator can show every conversation
/// Delta knows — whether or not it currently has a live pane — and route into
/// each one. Open/closed is process-runtime state from the registry, not a
/// persisted column, so it is computed per call rather than read off `session`.
#[derive(Debug, Clone)]
pub struct SessionListing {
    /// The persisted session record.
    pub session: Session,
    /// Whether the session currently has a live pane (is resumable into without
    /// a `--resume`). A closed session still appears, just with `open: false`.
    pub open: bool,
    /// The id of the session's trunk (`main`) thread, for drilling in.
    pub main_thread_id: ThreadId,
    /// The timestamp of the session's most recent message (ISO-8601 UTC), or
    /// `None` when the session has no messages yet. Derived per call from
    /// `MAX(message.created_at)`, so it reflects the latest activity even though
    /// it is not a persisted column on `session`.
    pub last_activity_at: Option<String>,
}
