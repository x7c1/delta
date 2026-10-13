//! Forgetting a directory's workspace trust.

use super::*;

#[tokio::test]
async fn forget_dir_trusted_removes_only_that_project_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join(".claude.json");
    let git = Git::with_config_path(config.clone(), Vec::new());
    git.ensure_dir_trusted("/worktrees/a").await.unwrap();
    git.ensure_dir_trusted("/worktrees/b").await.unwrap();

    git.forget_dir_trusted("/worktrees/a").await.unwrap();

    let value: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read(&config).await.unwrap()).unwrap();
    let projects = value["projects"].as_object().unwrap();
    assert_eq!(
        projects.keys().collect::<Vec<_>>(),
        vec!["/worktrees/b"],
        "exactly one projects key is gone"
    );
}
