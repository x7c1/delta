//! Finding the worktree a branch is checked out in, and checking an existing
//! branch out in a new worktree.

use super::*;

/// Clone `origin_path` into a fresh temp dir, returning the clone path.
async fn clone_into(origin_path: &str, parent: &std::path::Path) -> String {
    let clone_path = parent.join("clone").to_string_lossy().into_owned();
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
    clone_path
}

#[tokio::test]
async fn worktree_path_for_branch_finds_a_secondary_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let repo_root = tmp.path().to_str().unwrap().to_owned();
    let git = Git::new(Vec::new());

    // Check `feature` out in a secondary worktree.
    let wt_dir = tempfile::tempdir().unwrap();
    let wt_path = wt_dir
        .path()
        .join("feature-wt")
        .to_string_lossy()
        .into_owned();
    git_ok(&repo_root, &["worktree", "add", "-b", "feature", &wt_path]).await;

    let found = git
        .worktree_path_for_branch(&repo_root, "feature")
        .await
        .unwrap()
        .expect("feature is checked out in the secondary worktree");
    assert_eq!(canonical(&found).await, canonical(&wt_path).await);
}

#[tokio::test]
async fn worktree_path_for_branch_finds_the_main_working_tree() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let repo_root = tmp.path().to_str().unwrap().to_owned();
    let git = Git::new(Vec::new());

    // `main` is checked out in the main working tree (the repo root itself).
    let found = git
        .worktree_path_for_branch(&repo_root, "main")
        .await
        .unwrap()
        .expect("main is the main working tree's branch");
    assert_eq!(canonical(&found).await, canonical(&repo_root).await);
}

#[tokio::test]
async fn worktree_path_for_branch_is_none_when_not_checked_out() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let repo_root = tmp.path().to_str().unwrap().to_owned();
    // A local branch that exists but is not checked out anywhere.
    git_ok(&repo_root, &["branch", "idle"]).await;
    let git = Git::new(Vec::new());

    let found = git
        .worktree_path_for_branch(&repo_root, "idle")
        .await
        .unwrap();
    assert!(
        found.is_none(),
        "an unchecked-out branch has no worktree path"
    );
}

#[tokio::test]
async fn add_worktree_checkout_uses_an_existing_local_branch() {
    // Origin with `main` and `feature`; the clone fetches both.
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let origin_path = origin.path().to_str().unwrap();
    git_ok(origin_path, &["branch", "feature"]).await;

    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_into(origin_path, clone_parent.path()).await;
    // Create the local branch so the "existing local branch" path is taken.
    git_ok(&clone_path, &["branch", "feature", "origin/feature"]).await;

    let git = Git::new(Vec::new());
    let wt_dir = tempfile::tempdir().unwrap();
    let wt_path = wt_dir
        .path()
        .join("feature-wt")
        .to_string_lossy()
        .into_owned();

    git.add_worktree_checkout(&clone_path, &wt_path, "feature")
        .await
        .unwrap();

    // The worktree checked out `feature` itself (not a `delta-<id>` branch).
    let head = git_ok(&wt_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    assert_eq!(head, "feature");
}

#[tokio::test]
async fn add_worktree_checkout_creates_a_tracking_branch_from_origin() {
    // Origin with a `feature` branch the clone has not checked out locally.
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let origin_path = origin.path().to_str().unwrap();
    git_ok(origin_path, &["branch", "feature"]).await;

    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_into(origin_path, clone_parent.path()).await;
    // No local `feature` branch yet: the tracking-branch path is taken.

    let git = Git::new(Vec::new());
    let wt_dir = tempfile::tempdir().unwrap();
    let wt_path = wt_dir
        .path()
        .join("feature-wt")
        .to_string_lossy()
        .into_owned();

    git.add_worktree_checkout(&clone_path, &wt_path, "feature")
        .await
        .unwrap();

    // The worktree is on a local `feature` branch tracking `origin/feature`.
    let head = git_ok(&wt_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    assert_eq!(head, "feature");
    let upstream = git_ok(
        &wt_path,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .await;
    assert_eq!(upstream, "origin/feature");
}
