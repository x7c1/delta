//! `GET /api/sessions`: what each listed session carries.

use axum::http::StatusCode;

use delta_model::SessionId;
use delta_sqlite::SqliteStore;
use delta_usecase::{NewSession, SessionRecordStore, ThreadStore};

use crate::support::{build_app_with_store, get};

/// Every item of `GET /api/sessions` carries exactly its own session's threads,
/// in the shape and order `GET /api/sessions/{id}/threads` returns them — on the
/// first page and on a later one alike.
#[tokio::test]
async fn session_list_items_carry_their_sessions_threads() {
    let store = SqliteStore::open_in_memory().unwrap();
    let mut branched = Vec::new();
    for (id, branches) in [("sess-a", 2), ("sess-b", 0), ("sess-c", 1)] {
        let session_id = SessionId::from(id);
        let (_, main) = store
            .register_session(NewSession {
                id: session_id.clone(),
                cwd: "/work/delta".into(),
                transcript_path: format!("/tmp/{id}.jsonl"),
                branch_at_launch: None,
                repo_root: None,
                repository_display_name: None,
            })
            .await
            .unwrap();
        let mut parent = main;
        for n in 0..branches {
            // Each branch hangs off the previous one, so a tree deeper than one
            // level has to come through intact.
            parent = store
                .create_thread(&session_id, &format!("{id} branch {n}"), Some(parent))
                .await
                .unwrap()
                .id;
            branched.push(id);
        }
    }
    let (app, _, _, _) = build_app_with_store(store);

    // Walk the list one session per page, so the second and third sessions
    // arrive on later pages.
    let mut items = Vec::new();
    let mut uri = "/api/sessions?limit=1".to_owned();
    loop {
        let (status, body) = get(&app, &uri).await;
        assert_eq!(status, StatusCode::OK);
        let page = body["sessions"].as_array().expect("sessions array");
        // A full page always yields a cursor, so the walk ends on an empty one.
        assert!(page.len() <= 1, "at most one closed session per page");
        items.extend(page.iter().cloned());
        match body["next_cursor"].as_str() {
            Some(cursor) => uri = format!("/api/sessions?limit=1&cursor={cursor}"),
            None => break,
        }
    }
    assert_eq!(items.len(), 3, "every session is listed once");

    for item in &items {
        let id = item["session"]["id"].as_str().expect("session id");
        let (status, body) = get(&app, &format!("/api/sessions/{id}/threads")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            item["threads"], body["threads"],
            "{id}'s listed threads match its threads endpoint"
        );
        let threads = item["threads"].as_array().expect("threads array");
        assert_eq!(
            threads.len(),
            1 + branched.iter().filter(|b| **b == id).count(),
            "{id} carries its trunk and its own branches only"
        );
        assert!(threads.iter().all(|t| t["session_id"] == id));
        assert_eq!(threads[0]["id"], item["main_thread_id"], "the trunk leads");
    }
}
