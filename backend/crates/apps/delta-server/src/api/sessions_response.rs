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
}

impl From<SessionListing> for SessionListItem {
    fn from(listing: SessionListing) -> Self {
        SessionListItem {
            session: listing.session,
            open: listing.open,
            main_thread_id: listing.main_thread_id,
        }
    }
}

/// Response for `GET /api/sessions`: every known session, ordered by creation.
#[derive(Debug, Serialize)]
pub struct SessionsResponse {
    pub sessions: Vec<SessionListItem>,
}
