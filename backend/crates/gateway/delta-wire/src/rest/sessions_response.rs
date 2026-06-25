//! Response for `GET /api/sessions`.

use delta_usecase::SessionListing;
use serde::Serialize;
use ts_rs::TS;

use crate::session::WireSession;

/// One session in the list: the stored record plus its live state and trunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SessionListItem")]
pub struct WireSessionListItem {
    pub session: WireSession,
    /// Whether the session currently has a live pane (resumable without
    /// `--resume`). A closed session still appears, with `open: false`.
    pub open: bool,
    pub main_thread_id: i64,
    /// Timestamp of the session's most recent message (ISO-8601 UTC), or `null`
    /// when the session has no messages yet.
    pub last_activity_at: Option<String>,
}

impl From<SessionListing> for WireSessionListItem {
    fn from(listing: SessionListing) -> Self {
        WireSessionListItem {
            session: listing.session.into(),
            open: listing.open,
            main_thread_id: listing.main_thread_id.0,
            last_activity_at: listing.last_activity_at,
        }
    }
}

/// Response for `GET /api/sessions`: one page of sessions, most-recently-active
/// first, plus the cursor to fetch the following page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SessionsResponse")]
pub struct WireSessionsResponse {
    pub sessions: Vec<WireSessionListItem>,
    /// An opaque token to fetch the next page (echo it back as the `cursor`
    /// query parameter), or `null` when this is the last page.
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::{Session, SessionId, SessionStatus, ThreadId};

    #[test]
    fn a_page_serializes_with_the_rest_field_names() {
        let listing = SessionListing {
            session: Session {
                id: SessionId::from("sess-1"),
                cwd: "/work".into(),
                transcript_path: Some("/tmp/t.jsonl".into()),
                title: Some("title".into()),
                status: SessionStatus::Active,
                created_at: "2026-01-01T00:00:00Z".into(),
                branch_at_launch: None,
                repo_root: None,
                repository_display_name: None,
            },
            open: true,
            main_thread_id: ThreadId(1),
            last_activity_at: None,
        };
        assert_eq!(
            serde_json::to_value(WireSessionsResponse {
                sessions: vec![listing.into()],
                next_cursor: Some("abc".into()),
            })
            .unwrap(),
            serde_json::json!({
                "sessions": [{
                    "session": {
                        "id": "sess-1",
                        "cwd": "/work",
                        "transcript_path": "/tmp/t.jsonl",
                        "title": "title",
                        "status": "active",
                        "created_at": "2026-01-01T00:00:00Z",
                        "branch_at_launch": null,
                        "repo_root": null,
                        "repository_display_name": null,
                    },
                    "open": true,
                    "main_thread_id": 1,
                    "last_activity_at": null,
                }],
                "next_cursor": "abc",
            }),
        );
    }
}
