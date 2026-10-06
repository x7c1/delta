//! The bulk session removal routes: `GET` and `POST /api/sessions/prune`.

use super::*;
use axum::http::StatusCode;
use delta_usecase::{NewSession, SessionEvent, SessionId, SessionStore};

/// Register a closed session that ran in a directory of its own (not a Delta
/// worktree, so its removal touches no git).
async fn closed_session(state: &AppState, id: &str) {
    state
        .interactor()
        .store()
        .register_session(NewSession {
            id: id.into(),
            cwd: format!("/tmp/delta-prune-test/{id}"),
            transcript_path: format!("/tmp/{id}.jsonl"),
            branch_at_launch: None,
            repo_root: None,
            repository_display_name: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn the_preview_counts_what_the_prune_then_removes_and_each_removal_is_broadcast() {
    let state = test_state().await;
    closed_session(&state, "sess-a").await;
    closed_session(&state, "sess-b").await;
    let mut events = state.subscribe();

    let (status, preview) = request_json(
        &state,
        "GET",
        "/api/sessions/prune?older_than_days=0&statuses=ended,failed",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["count"], 2);
    let (status, too_old) = request_json(
        &state,
        "GET",
        "/api/sessions/prune?older_than_days=30",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(too_old["count"], 0, "nothing is 30 days old yet");
    let (_, failed_only) = request_json(
        &state,
        "GET",
        "/api/sessions/prune?older_than_days=0&statuses=failed",
        None,
    )
    .await;
    assert_eq!(
        failed_only["count"], 0,
        "neither session is a failed launch"
    );

    let (status, report) = request_json(
        &state,
        "POST",
        "/api/sessions/prune",
        Some(r#"{ "older_than_days": 0 }"#),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["removed"], 2);
    assert_eq!(report["skipped"], serde_json::json!([]));
    assert_eq!(report["kept"], serde_json::json!([]));
    let mut removed = Vec::new();
    for _ in 0..2 {
        match events.try_recv().unwrap() {
            SessionEvent::SessionRemoved { session_id } => removed.push(session_id),
            other => panic!("expected session_removed, got {other:?}"),
        }
    }
    removed.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    assert_eq!(
        removed,
        vec![SessionId::from("sess-a"), SessionId::from("sess-b")]
    );
    for id in ["sess-a", "sess-b"] {
        assert!(state
            .interactor()
            .store()
            .session(&SessionId::from(id))
            .await
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn a_malformed_prune_request_is_a_client_error() {
    let state = test_state().await;

    let (unknown_status, _) = request_json(
        &state,
        "GET",
        "/api/sessions/prune?older_than_days=0&statuses=open",
        None,
    )
    .await;
    let (missing_age, _) = request_json(&state, "GET", "/api/sessions/prune", None).await;
    let (negative_age, _) = request_json(
        &state,
        "POST",
        "/api/sessions/prune",
        Some(r#"{ "older_than_days": -1 }"#),
    )
    .await;

    assert_eq!(unknown_status, StatusCode::BAD_REQUEST);
    assert_eq!(missing_age, StatusCode::BAD_REQUEST);
    assert!(
        negative_age.is_client_error(),
        "a negative age is rejected, got {negative_age}"
    );
}
