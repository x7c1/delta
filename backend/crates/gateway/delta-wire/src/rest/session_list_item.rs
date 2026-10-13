//! One session in the list: the stored record plus its live state and trunk.

use delta_usecase::SessionListing;
use serde::Serialize;
use ts_rs::TS;

use crate::session::WireSession;
use crate::thread::WireThread;

/// One session in the list: the stored record plus its live state and trunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SessionListItem")]
pub struct WireSessionListItem {
    pub session: WireSession,
    /// Whether the session currently has a live pane (resumable without
    /// `--resume`). A closed session still appears, with `open: false`.
    pub open: bool,
    /// Whether the session holds a pane the embedded terminal may attach to
    /// that nothing has bound yet — a launch whose pane is up while its first
    /// hook has not arrived. That is the window in which the launch may be
    /// waiting on an interactive prompt only a human can answer, so the row
    /// carries the fact and any browser (including one that reloaded mid-launch
    /// and never saw `spawn_pane_ready`) can offer the terminal. Mutually
    /// exclusive with `open`.
    pub pane_starting: bool,
    /// Whether the session is open on a pane Delta re-adopted after a restart
    /// but whose agent can no longer deliver hooks to this server, because the
    /// hook endpoint (port or secret) changed between the two runs. Its
    /// transcript and terminal still work; prompt echoes, turn ends and
    /// permission dialogs do not arrive. The browser shows a notice telling the
    /// user to use the terminal, or to close the session and send again (which
    /// resumes it with fresh settings). Always `false` for a session that is
    /// not open.
    pub hooks_unreachable: bool,
    pub main_thread_id: i64,
    /// Every thread of the session — the trunk and its branches — in the shape
    /// and order `GET /api/sessions/{id}/threads` returns them (ascending id).
    /// Carried on the row so a client can draw each session's thread tree from
    /// the list alone, without a request per session.
    pub threads: Vec<WireThread>,
    /// Timestamp of the session's most recent message (ISO-8601 UTC), or `null`
    /// when the session has no messages yet.
    pub last_activity_at: Option<String>,
}

impl From<SessionListing> for WireSessionListItem {
    fn from(listing: SessionListing) -> Self {
        WireSessionListItem {
            session: listing.session.into(),
            open: listing.open,
            pane_starting: listing.pane_starting,
            hooks_unreachable: listing.hooks_unreachable,
            main_thread_id: listing.main_thread_id.0,
            threads: listing.threads.into_iter().map(WireThread::from).collect(),
            last_activity_at: listing.last_activity_at,
        }
    }
}
