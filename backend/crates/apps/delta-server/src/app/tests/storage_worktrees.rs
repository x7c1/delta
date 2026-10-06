//! The worktree routes of Settings → Storage: `GET` and
//! `DELETE /api/storage/worktrees`, against a real repository.

use std::path::Path;
use std::process::Command;

use super::*;
use axum::http::StatusCode;

/// Run `git -C <dir> <args>`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git available");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A state whose worktree base is a fresh directory holding a dirty linked
/// worktree (`dirty`) of a repository outside it, and a plain directory git
/// does not know (`stray`). Returns the state, the base, and the temp dirs to
/// keep alive.
async fn state_with_worktrees() -> (AppState, String, [tempfile::TempDir; 2]) {
    let (repo, base) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.email", "test@example.com"],
        &["config", "user.name", "Test"],
        &["commit", "-q", "--allow-empty", "-m", "initial"],
    ] {
        git(repo.path(), args);
    }
    let dirty = base.path().join("dirty");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "delta-x",
            dirty.to_str().unwrap(),
        ],
    );
    std::fs::write(dirty.join("notes.txt"), "work").unwrap();
    std::fs::create_dir(base.path().join("stray")).unwrap();
    let base_path = base.path().to_string_lossy().into_owned();
    let state = AppState::build(&Config {
        worktree_base: base_path.clone(),
        ..test_config()
    })
    .await
    .unwrap();
    (state, base_path, [repo, base])
}

#[tokio::test]
async fn the_list_reports_what_git_says_of_each_directory() {
    let (state, base, _dirs) = state_with_worktrees().await;

    let (status, body) = request_json(&state, "GET", "/api/storage/worktrees", None).await;

    assert_eq!(status, StatusCode::OK);
    let worktrees = body["worktrees"].as_array().unwrap();
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[0]["path"], format!("{base}/dirty"));
    assert_eq!(worktrees[0]["in_use"], false);
    assert_eq!(worktrees[0]["dirty"], true);
    assert!(worktrees[0]["repo_root"].is_string());
    assert_eq!(worktrees[1]["path"], format!("{base}/stray"));
    assert_eq!(worktrees[1]["repo_root"], serde_json::Value::Null);
    assert_eq!(worktrees[1]["dirty"], serde_json::Value::Null);
}

#[tokio::test]
async fn a_removal_is_refused_without_force_and_goes_through_with_it() {
    let (state, base, _dirs) = state_with_worktrees().await;
    let body = |path: &str, force: bool| format!(r#"{{ "path": "{path}", "force": {force} }}"#);
    let (dirty, stray) = (format!("{base}/dirty"), format!("{base}/stray"));

    let (refused_dirty, dirty_body) = request_json(
        &state,
        "DELETE",
        "/api/storage/worktrees",
        Some(&body(&dirty, false)),
    )
    .await;
    let (refused_stray, stray_body) = request_json(
        &state,
        "DELETE",
        "/api/storage/worktrees",
        Some(&body(&stray, false)),
    )
    .await;
    let (refused_outside, outside_body) = request_json(
        &state,
        "DELETE",
        "/api/storage/worktrees",
        Some(&body("/etc", true)),
    )
    .await;

    assert_eq!(refused_dirty, StatusCode::CONFLICT);
    assert_eq!(dirty_body["code"], "worktree_dirty");
    assert_eq!(refused_stray, StatusCode::CONFLICT);
    assert_eq!(stray_body["code"], "worktree_not_registered");
    assert_eq!(refused_outside, StatusCode::CONFLICT);
    assert_eq!(outside_body["code"], "worktree_outside_base");
    assert!(
        Path::new(&dirty).join("notes.txt").exists(),
        "nothing was removed"
    );

    let (forced_dirty, _) = request_json(
        &state,
        "DELETE",
        "/api/storage/worktrees",
        Some(&body(&dirty, true)),
    )
    .await;
    let (forced_stray, _) = request_json(
        &state,
        "DELETE",
        "/api/storage/worktrees",
        Some(&body(&stray, true)),
    )
    .await;

    assert_eq!(forced_dirty, StatusCode::NO_CONTENT);
    assert_eq!(forced_stray, StatusCode::NO_CONTENT);
    assert!(!Path::new(&dirty).exists());
    assert!(!Path::new(&stray).exists());
    let (_, listing) = request_json(&state, "GET", "/api/storage/worktrees", None).await;
    assert_eq!(listing["worktrees"], serde_json::json!([]));
}
