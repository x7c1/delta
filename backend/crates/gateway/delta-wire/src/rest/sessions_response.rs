//! Response for `GET /api/sessions`.

use serde::Serialize;
use ts_rs::TS;

use crate::rest::session_list_item::WireSessionListItem;

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

    use delta_usecase::SessionListing;

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
