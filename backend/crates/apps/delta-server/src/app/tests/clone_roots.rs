//! Repository, clone-root and clone routes.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use std::sync::atomic::Ordering;
use tower::ServiceExt;

/// Register `path` as a clone root through the real endpoint, so the clone
/// tests set their fixture up the same way a user would.
async fn register_clone_root(app: &axum::Router, path: &str) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("POST")
                .uri("/api/clone-roots")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"path":"{path}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

/// The error body's `code`, for asserting on a machine-readable refusal.
async fn error_code(response: axum::response::Response) -> Option<String> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    body["code"].as_str().map(str::to_owned)
}

#[tokio::test]
async fn repositories_returns_an_empty_list_when_no_sessions() {
    // No sessions registered yet → no repositories. The endpoint
    // replies with `{ repositories: [] }`, not 404.
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .uri("/api/repositories")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["repositories"].as_array().unwrap().len(),
        0,
        "no sessions = no repositories"
    );
}

#[tokio::test]
async fn clone_roots_round_trip_create_list_delete() {
    let state = test_state().await;
    let app = router(state);

    // Empty on a fresh store.
    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .uri("/api/clone-roots")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["clone_roots"].as_array().unwrap().len(), 0);

    // Register one root.
    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("POST")
                .uri("/api/clone-roots")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/home/dev/projects/"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);
    let bytes = to_bytes(create.into_body(), usize::MAX).await.unwrap();
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // Trailing slash is trimmed for canonicalisation.
    assert_eq!(created["path"], "/home/dev/projects");

    // Listed.
    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .uri("/api/clone-roots")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["clone_roots"].as_array().unwrap().len(), 1);
    assert_eq!(body["clone_roots"][0]["path"], "/home/dev/projects");

    // Duplicate is a 409 with the stable error code.
    let dup = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("POST")
                .uri("/api/clone-roots")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/home/dev/projects"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dup.status(), StatusCode::CONFLICT);
    let bytes = to_bytes(dup.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "clone_root_duplicate");

    // Delete via the base64 path token.
    let token = crate::api::clone_root_path::encode("/home/dev/projects");
    let delete = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("DELETE")
                .uri(format!("/api/clone-roots/{token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    // The list is empty again.
    let list = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .uri("/api/clone-roots")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["clone_roots"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn clone_repository_rejects_an_unregistered_clone_root_and_starts_no_job() {
    let (state, clone_calls) = test_state_with_gh_stub().await;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap();
    // Note: never registered. The directory existing is not enough — Delta
    // writes clones only where the user said clones go.

    let response = router(state)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("POST")
                .uri("/api/repositories/clone")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"repo_owner":"x7c1","repo_name":"delta","clone_root":"{root}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        error_code(response).await.as_deref(),
        Some("clone_root_not_registered"),
    );
    assert_eq!(
        clone_calls.load(Ordering::SeqCst),
        0,
        "a refused request must start no clone job",
    );
}

#[tokio::test]
async fn clone_repository_rejects_an_existing_destination_with_409_and_starts_no_job() {
    let (state, clone_calls) = test_state_with_gh_stub().await;
    let app = router(state);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap();
    register_clone_root(&app, root).await;
    // `<root>/delta` is taken. There is no fallback naming, so the request
    // is refused rather than landing somewhere else.
    std::fs::create_dir(tmp.path().join("delta")).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("POST")
                .uri("/api/repositories/clone")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"repo_owner":"x7c1","repo_name":"delta","clone_root":"{root}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        error_code(response).await.as_deref(),
        Some("clone_dest_exists"),
    );
    assert_eq!(
        clone_calls.load(Ordering::SeqCst),
        0,
        "a refused request must start no clone job",
    );
}

#[tokio::test]
async fn clone_repository_accepts_a_registered_root_with_202() {
    let (state, _clone_calls) = test_state_with_gh_stub().await;
    let app = router(state);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap();
    register_clone_root(&app, root).await;

    let response = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("POST")
                .uri("/api/repositories/clone")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"repo_owner":"x7c1","repo_name":"delta","clone_root":"{root}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    // Accepted, not completed: the clone outlives this response and reports
    // on `/ws`.
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn create_clone_root_rejects_a_non_absolute_path() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("POST")
                .uri("/api/clone-roots")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"relative/path"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// A blank `path` is a `400`, whether it is empty, whitespace-only, or
/// spelled entirely with slashes. Registering `/` by accident is not
/// harmless: `GET /api/repositories` scans every registered root's depth-1
/// children on every call, so a `/` row would re-read the filesystem root
/// each time and list whichever top-level directories happen to be clones.
#[tokio::test]
async fn create_clone_root_rejects_a_blank_path() {
    for path in ["", "   ", "//", "///"] {
        let response = router(test_state().await)
            .oneshot(
                Request::builder()
                    .header("host", "127.0.0.1")
                    .method("POST")
                    .uri("/api/clone-roots")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"path":"{path}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "expected a 400 for the blank path {path:?}",
        );
    }
}

/// The bare root is non-blank and absolute, so rejecting blanks must not
/// take it with them: `/` stays a registrable clone root.
#[tokio::test]
async fn create_clone_root_accepts_the_filesystem_root() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("POST")
                .uri("/api/clone-roots")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["path"].as_str(), Some("/"));
}

/// A trailing slash is still canonicalised away, so the user-typed
/// `/home/dev/projects/` and `/home/dev/projects` stay the same row.
#[tokio::test]
async fn create_clone_root_canonicalises_a_trailing_slash() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("POST")
                .uri("/api/clone-roots")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/home/dev/projects/"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["path"].as_str(), Some("/home/dev/projects"));
}

#[tokio::test]
async fn delete_unknown_clone_root_is_idempotent() {
    // No registration first. The DELETE replies 204 anyway: a Settings
    // dialog click on an unknown path is the user's intent ("ensure gone"),
    // not a precondition.
    let token = crate::api::clone_root_path::encode("/never/registered");
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .method("DELETE")
                .uri(format!("/api/clone-roots/{token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}
