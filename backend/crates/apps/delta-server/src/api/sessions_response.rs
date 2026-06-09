//! Response for `GET /api/sessions`.

use serde::Serialize;

use delta_usecase::{Session, SessionListing, ThreadId};

/// One session in the list: the stored record plus its live state and trunk.
#[derive(Debug, Serialize)]
pub struct SessionListItem {
    pub session: Session,
    /// Whether the session currently has a live pane (resumable without
    /// `--resume`). A closed session still appears, with `open: false`.
    pub open: bool,
    pub main_thread_id: ThreadId,
    /// Timestamp of the session's most recent message (ISO-8601 UTC), or `null`
    /// when the session has no messages yet.
    pub last_activity_at: Option<String>,
}

impl From<SessionListing> for SessionListItem {
    fn from(listing: SessionListing) -> Self {
        SessionListItem {
            session: listing.session,
            open: listing.open,
            main_thread_id: listing.main_thread_id,
            last_activity_at: listing.last_activity_at,
        }
    }
}

/// Response for `GET /api/sessions`: one page of sessions, most-recently-active
/// first, plus the cursor to fetch the following page.
#[derive(Debug, Serialize)]
pub struct SessionsResponse {
    pub sessions: Vec<SessionListItem>,
    /// An opaque token to fetch the next page (echo it back as the `cursor`
    /// query parameter), or `null` when this is the last page.
    pub next_cursor: Option<String>,
}
