//! Working-directory browsing, git and open-cwd routes.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn workdir_list_browses_a_real_directory() {
    // Browse a temp directory containing one subdirectory and one file:
    // only the subdirectory should appear.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("child")).unwrap();
    std::fs::write(dir.path().join("a-file"), "x").unwrap();

    // tempdir paths contain no query-reserved characters, so they need no
    // percent-encoding for this test.
    let uri = format!("/api/workdir/list?path={}", dir.path().to_str().unwrap());
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri(&uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "dirs only, files excluded");
    assert_eq!(entries[0]["name"], "child");
    assert!(body["parent"].is_string(), "a non-root dir has a parent");
}

#[tokio::test]
async fn workdir_list_rejects_a_missing_path_with_400() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/workdir/list?path=/no/such/path/here")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workdir_recent_returns_an_empty_list_when_no_sessions() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/workdir/recent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["workdirs"].as_array().unwrap().len(),
        0,
        "no sessions yet means no recent workdirs"
    );
}

/// A whitespace-only `path` is blank, so it is refused at the query boundary
/// rather than handed to git. (`require_path`'s trimming and its
/// missing/empty cases are unit-tested in `crate::api`; these two tests pin
/// that each endpoint actually routes through it.)
#[tokio::test]
async fn workdir_git_rejects_a_blank_path() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/workdir/git?path=%20%20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workdir_git_branches_rejects_a_blank_path() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/workdir/git/branches?path=%20%20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn open_cwd_rejects_a_path_not_in_the_allowlist_with_400() {
    // No sessions registered yet → no known cwds. A `POST /api/open-cwd`
    // for any path must be rejected with the stable code, and the router
    // must not have to reach the (unwired) opener stub either — the
    // allowlist check runs first.
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/open-cwd")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/etc/passwd"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body.get("code").and_then(serde_json::Value::as_str),
        Some("open_cwd_path_not_allowed"),
    );
}

#[tokio::test]
async fn open_cwd_rejects_an_unknown_handler_with_400() {
    // Register a session so the path is in the allowlist and the check
    // moves on to the handler resolution.
    let state = test_state().await;
    let app = router(state.clone());
    let submit = serde_json::json!({
        "prompt": "seed",
        "session_id": "sess-1",
        "transcript_path": "/tmp/none.jsonl",
        "cwd": "/projects/known"
    })
    .to_string();
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/hooks/user-prompt-submit")
                .header("content-type", "application/json")
                .body(Body::from(submit))
                .unwrap(),
        )
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/open-cwd")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"path":"/projects/known","handler":"emacs"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body.get("code").and_then(serde_json::Value::as_str),
        Some("open_cwd_unknown_handler"),
    );
}

#[tokio::test]
async fn open_cwd_rejects_a_blank_path_with_400() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/open-cwd")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
