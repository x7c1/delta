//! Storage inventory route.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn storage_reports_the_configured_data_dir_and_the_database_on_disk() {
    let config = test_config();
    let state = AppState::build(&config).await.unwrap();

    // Kept alive until the files are measured: dropping the last handle closes
    // the database, which checkpoints and removes its `-wal`/`-shm`.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/storage")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let data_dir = std::path::Path::new(&config.data_dir);
    assert_eq!(body["data_dir"], config.data_dir);
    assert_eq!(body["identifier"], config.identifier);
    assert_eq!(body["tmux_socket"], config.tmux_socket);
    assert_eq!(body["worktree_base"], config.worktree_base);
    assert_eq!(
        body["database"]["path"],
        data_dir.join("delta.db").to_string_lossy().as_ref(),
    );

    // Building the state opened the database, so `delta.db` exists; its size
    // is what the files on disk add up to, sidecars included.
    let on_disk: u64 = ["delta.db", "delta.db-wal", "delta.db-shm"]
        .iter()
        .filter_map(|name| std::fs::metadata(data_dir.join(name)).ok())
        .map(|metadata| metadata.len())
        .sum();
    assert!(on_disk > 0, "the opened database should be on disk");
    assert_eq!(body["database"]["bytes"], on_disk);

    // A fresh database was never migrated destructively.
    assert_eq!(body["snapshots"], serde_json::json!([]));
    drop(state);

    // Paths only: neither secret is served.
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains(TEST_AUTH_TOKEN));
    assert!(!text.contains(TEST_HOOK_SECRET));
}
