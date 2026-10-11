//! Response for `GET /api/sessions`.

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

/// Response for `GET /api/sessions`: one page of sessions, open-first (every
/// live session, then the closed ones, each group most-recently-active first),
/// plus the cursor to fetch the following page. The first page carries the whole
/// live group; the cursor walks the closed ones.
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

    use delta_model::{AgentProvider, Session, SessionId, SessionStatus, Thread, ThreadId};

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
                requested_workdir: None,
                repository_display_name: None,
                provider: AgentProvider::Claude,
                provider_session_id: None,
                provider_thread_id: None,
                pull_request_number: None,
                failure_reason: None,
            },
            open: true,
            pane_starting: false,
            hooks_unreachable: false,
            main_thread_id: ThreadId(1),
            threads: vec![Thread {
                id: ThreadId(1),
                session_id: SessionId::from("sess-1"),
                title: "main".into(),
                parent_thread_id: None,
                root_message_uuid: None,
                created_at: "2026-01-01T00:00:00Z".into(),
                last_activity_at: None,
            }],
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
                        "provider": "claude",
                        "provider_session_id": null,
                        "provider_thread_id": null,
                        "pull_request_number": null,
                        "failure_reason": null,
                    },
                    "open": true,
                    "pane_starting": false,
                    "hooks_unreachable": false,
                    "main_thread_id": 1,
                    "threads": [{
                        "id": 1,
                        "session_id": "sess-1",
                        "title": "main",
                        "parent_thread_id": null,
                        "root_message_uuid": null,
                        "created_at": "2026-01-01T00:00:00Z",
                        "last_activity_at": null,
                    }],
                    "last_activity_at": null,
                }],
                "next_cursor": "abc",
            }),
        );
    }
}
