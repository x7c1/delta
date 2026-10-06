//! `DELETE /api/storage/snapshots`.

use super::*;
use axum::http::StatusCode;

#[tokio::test]
async fn a_listed_snapshot_is_deleted_and_any_other_path_is_not_found() {
    let config = test_config();
    let state = AppState::build(&config).await.unwrap();
    let data_dir = std::path::Path::new(&config.data_dir);
    let snapshot = data_dir.join("delta.db.bak-v1");
    std::fs::write(&snapshot, b"snapshot").unwrap();
    let snapshot = snapshot.to_string_lossy().into_owned();
    let database = data_dir.join("delta.db").to_string_lossy().into_owned();
    let body = |path: &str| format!(r#"{{ "path": "{path}" }}"#);

    let (refused, _) = request_json(
        &state,
        "DELETE",
        "/api/storage/snapshots",
        Some(&body(&database)),
    )
    .await;
    let (deleted, _) = request_json(
        &state,
        "DELETE",
        "/api/storage/snapshots",
        Some(&body(&snapshot)),
    )
    .await;
    let (again, _) = request_json(
        &state,
        "DELETE",
        "/api/storage/snapshots",
        Some(&body(&snapshot)),
    )
    .await;

    assert_eq!(
        refused,
        StatusCode::NOT_FOUND,
        "the database is not a snapshot"
    );
    assert!(std::path::Path::new(&database).exists());
    assert_eq!(deleted, StatusCode::NO_CONTENT);
    assert!(!std::path::Path::new(&snapshot).exists());
    assert_eq!(again, StatusCode::NOT_FOUND);
    let (_, storage) = request_json(&state, "GET", "/api/storage", None).await;
    assert_eq!(storage["snapshots"], serde_json::json!([]));
}
