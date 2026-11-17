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

/// Check the existing local `branch` out in a secondary worktree of
/// `repo_root`, then delete that worktree's directory without telling git,
/// leaving a `prunable` registration behind.
async fn check_out_in_vanished_worktree(repo_root: &str, branch: &str) {
    let wt_dir = tempfile::tempdir().unwrap();
    let wt_path = wt_dir.path().join("gone-wt").to_string_lossy().into_owned();
    git_ok(repo_root, &["worktree", "add", &wt_path, branch]).await;
    drop(wt_dir);
    assert!(!std::path::Path::new(&wt_path).exists());
}

#[tokio::test]
async fn worktree_path_for_branch_skips_a_worktree_whose_directory_is_gone() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let repo_root = tmp.path().to_str().unwrap().to_owned();
    git_ok(&repo_root, &["branch", "feature"]).await;
    check_out_in_vanished_worktree(&repo_root, "feature").await;
    let git = Git::new(Vec::new());

    let found = git
        .worktree_path_for_branch(&repo_root, "feature")
        .await
        .unwrap();
    assert!(
        found.is_none(),
        "a prunable worktree must not be reported, got {found:?}"
    );
}

#[tokio::test]
async fn add_worktree_checkout_prunes_a_vanished_worktree_holding_the_branch() {
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let origin_path = origin.path().to_str().unwrap();
    git_ok(origin_path, &["branch", "feature"]).await;

    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_into(origin_path, clone_parent.path()).await;
    git_ok(&clone_path, &["branch", "feature", "origin/feature"]).await;
    check_out_in_vanished_worktree(&clone_path, "feature").await;

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

    let head = git_ok(&wt_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    assert_eq!(head, "feature");
    let found = git
        .worktree_path_for_branch(&clone_path, "feature")
        .await
        .unwrap()
        .expect("feature is checked out in the new worktree");
    assert_eq!(canonical(&found).await, canonical(&wt_path).await);
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

#[tokio::test]
async fn restore_worktree_checkout_rebuilds_a_deleted_worktree_at_the_same_path() {
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let origin_path = origin.path().to_str().unwrap();
    git_ok(origin_path, &["branch", "feature"]).await;

    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_into(origin_path, clone_parent.path()).await;

    let git = Git::new(Vec::new());
    let base = tempfile::tempdir().unwrap();
    let parent = base.path().join("nested");
    let wt_path = parent.join("feature-wt").to_string_lossy().into_owned();
    git.add_worktree_checkout(&clone_path, &wt_path, "feature")
        .await
        .unwrap();

    // The worktree's directory — its parent included — is deleted behind
    // git's back, as `rm -rf` would; the branch lives on.
    std::fs::remove_dir_all(&parent).unwrap();
    assert!(!std::path::Path::new(&wt_path).exists());

    git.restore_worktree_checkout(&clone_path, &wt_path, "feature")
        .await
        .unwrap();

    let head = git_ok(&wt_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    assert_eq!(head, "feature", "the rebuilt directory is on the branch");
    let found = git
        .worktree_path_for_branch(&clone_path, "feature")
        .await
        .unwrap()
        .expect("feature is checked out in the rebuilt worktree");
    assert_eq!(canonical(&found).await, canonical(&wt_path).await);
}

/// A clone with a local branch holding a commit never pushed, and an `origin`
/// that can no longer be reached.
async fn clone_with_unpushed_branch_and_unreachable_origin(
    parent: &std::path::Path,
    branch: &str,
) -> (String, String) {
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let clone_path = clone_into(origin.path().to_str().unwrap(), parent).await;
    git_ok(&clone_path, &["config", "user.email", "test@example.com"]).await;
    git_ok(&clone_path, &["config", "user.name", "Test"]).await;
    git_ok(&clone_path, &["checkout", "-q", "-b", branch]).await;
    git_ok(
        &clone_path,
        &["commit", "-q", "--allow-empty", "-m", "unpushed"],
    )
    .await;
    let unpushed = git_ok(&clone_path, &["rev-parse", "HEAD"]).await;
    git_ok(&clone_path, &["checkout", "-q", "main"]).await;
    drop(origin);
    (clone_path, unpushed)
}

#[tokio::test]
async fn restore_worktree_checkout_takes_the_local_branch_without_origin() {
    let clone_parent = tempfile::tempdir().unwrap();
    let (clone_path, unpushed) =
        clone_with_unpushed_branch_and_unreachable_origin(clone_parent.path(), "local-only").await;

    let git = Git::new(Vec::new());
    let wt_dir = tempfile::tempdir().unwrap();
    let wt_path = wt_dir.path().join("wt").to_string_lossy().into_owned();
    git.restore_worktree_checkout(&clone_path, &wt_path, "local-only")
        .await
        .unwrap();

    let head = git_ok(&wt_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    assert_eq!(head, "local-only");
    let commit = git_ok(&wt_path, &["rev-parse", "HEAD"]).await;
    assert_eq!(commit, unpushed, "the unpushed commit is restored");
}

#[tokio::test]
async fn restore_worktree_checkout_tracks_a_branch_left_only_on_origin() {
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let origin_path = origin.path().to_str().unwrap();
    git_ok(origin_path, &["branch", "feature"]).await;
    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_into(origin_path, clone_parent.path()).await;

    let git = Git::new(Vec::new());
    let wt_dir = tempfile::tempdir().unwrap();
    let wt_path = wt_dir.path().join("wt").to_string_lossy().into_owned();
    git.restore_worktree_checkout(&clone_path, &wt_path, "feature")
        .await
        .unwrap();

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

#[tokio::test]
async fn add_worktree_checkout_fails_when_the_fetch_fails_even_with_a_local_branch() {
    // The pull-request launch path always fetches first, and a failed fetch
    // fails it, local branch or not.
    let clone_parent = tempfile::tempdir().unwrap();
    let (clone_path, _) =
        clone_with_unpushed_branch_and_unreachable_origin(clone_parent.path(), "local-only").await;

    let git = Git::new(Vec::new());
    let wt_dir = tempfile::tempdir().unwrap();
    let wt_path = wt_dir.path().join("wt").to_string_lossy().into_owned();
    let err = git
        .add_worktree_checkout(&clone_path, &wt_path, "local-only")
        .await
        .expect_err("the fetch fails");
    assert!(
        err.to_string().contains("fetch origin"),
        "the fetch's failure is the error, got: {err}"
    );
    assert!(!std::path::Path::new(&wt_path).exists());
}

#[tokio::test]
async fn add_worktree_checkout_fails_for_a_branch_that_exists_nowhere() {
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let origin_path = origin.path().to_str().unwrap();

    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_into(origin_path, clone_parent.path()).await;

    let git = Git::new(Vec::new());
    let wt_dir = tempfile::tempdir().unwrap();
    let wt_path = wt_dir.path().join("wt").to_string_lossy().into_owned();
    let err = git
        .add_worktree_checkout(&clone_path, &wt_path, "nowhere")
        .await
        .expect_err("there is nothing to check out");
    assert!(
        err.to_string().contains("fetch origin"),
        "the fetch's failure is the error, got: {err}"
    );
    assert!(!std::path::Path::new(&wt_path).exists());
}

#[tokio::test]
async fn has_branch_sees_local_and_remote_tracking_branches_only() {
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let origin_path = origin.path().to_str().unwrap();
    git_ok(origin_path, &["branch", "remote-only"]).await;

    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_into(origin_path, clone_parent.path()).await;
    git_ok(&clone_path, &["branch", "local-only"]).await;

    let git = Git::new(Vec::new());
    assert!(git.has_branch(&clone_path, "local-only").await.unwrap());
    assert!(
        git.has_branch(&clone_path, "remote-only").await.unwrap(),
        "origin/remote-only counts"
    );
    assert!(!git.has_branch(&clone_path, "nowhere").await.unwrap());
}
