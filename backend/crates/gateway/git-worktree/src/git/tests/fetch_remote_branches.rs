//! Listing the branches `origin` has.

use super::*;

/// A local "remote": a second clone whose `origin` is the first repo, so
/// fetch/`origin/<branch>` work fully offline.
#[tokio::test]
async fn fetch_remote_branches_lists_origin_branches_offline() {
    // Origin repo with a `main` and a `feature` branch.
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let origin_path = origin.path().to_str().unwrap();
    let status = Command::new("git")
        .arg("-C")
        .arg(origin_path)
        .args(["branch", "feature"])
        .output()
        .await
        .unwrap();
    assert!(status.status.success());

    // Clone it so the clone has `origin` pointing at the first repo and an
    // `origin/HEAD` symref recording the default branch.
    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_parent
        .path()
        .join("clone")
        .to_string_lossy()
        .into_owned();
    let status = Command::new("git")
        .args(["clone", "-q", origin_path, &clone_path])
        .output()
        .await
        .unwrap();
    assert!(
        status.status.success(),
        "clone failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    let git = Git::new(Vec::new());
    let remote = git.fetch_remote_branches(&clone_path).await.unwrap();

    assert!(
        remote.branches.contains(&"main".to_owned()),
        "main is a remote branch, got {:?}",
        remote.branches
    );
    assert!(
        remote.branches.contains(&"feature".to_owned()),
        "feature is a remote branch, got {:?}",
        remote.branches
    );
    assert!(
        !remote.branches.iter().any(|b| b == "HEAD"),
        "the origin/HEAD symref is excluded, got {:?}",
        remote.branches
    );
    assert_eq!(
        remote.default_branch.as_deref(),
        Some("main"),
        "the clone's origin/HEAD records main as the default branch"
    );

    // A worktree off the remote `feature` branch starts from origin/feature.
    let worktree_dir = tempfile::tempdir().unwrap();
    let worktree_path = worktree_dir
        .path()
        .join("delta-remote")
        .to_string_lossy()
        .into_owned();
    git.create_worktree(
        &clone_path,
        &worktree_path,
        "delta-remote",
        WorktreeStartPoint::RemoteBranch("feature".to_owned()),
    )
    .await
    .unwrap();
    assert!(
        tokio::fs::metadata(&worktree_path).await.unwrap().is_dir(),
        "a remote-branch worktree was created"
    );
}
