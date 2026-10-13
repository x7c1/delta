//! Creating a worktree on a fresh branch.

use super::*;

#[tokio::test]
async fn create_worktree_from_head_produces_a_worktree_and_branch() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let repo_root = tmp.path().to_str().unwrap().to_owned();
    let git = Git::new(Vec::new());

    // Create the worktree outside the repo dir so it is a sibling path.
    let worktree_dir = tempfile::tempdir().unwrap();
    let worktree_path = worktree_dir
        .path()
        .join("delta-session-1")
        .to_string_lossy()
        .into_owned();

    git.create_worktree(
        &repo_root,
        &worktree_path,
        "delta-session-1",
        WorktreeStartPoint::Head,
    )
    .await
    .unwrap();

    // The worktree directory exists and is itself a git working tree whose
    // root is the worktree path.
    assert!(
        tokio::fs::metadata(&worktree_path).await.unwrap().is_dir(),
        "the worktree directory was created"
    );
    let wt_root = git
        .repo_root(&worktree_path)
        .await
        .unwrap()
        .expect("the worktree is a git working tree");
    let expected = tokio::fs::canonicalize(&worktree_path)
        .await
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let actual = tokio::fs::canonicalize(&wt_root)
        .await
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(actual, expected);

    // The new branch is checked out in the worktree.
    let head = Command::new("git")
        .arg("-C")
        .arg(&worktree_path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await
        .unwrap();
    assert_eq!(trimmed_stdout(&head), "delta-session-1");
}

#[tokio::test]
async fn create_worktree_creates_the_missing_parent_directory() {
    // The worktree base (e.g. `$HOME/.delta/worktrees`) may not exist yet on
    // a fresh install; `git worktree add` would fail if its parent is
    // absent. The gateway must create the parent chain first.
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let repo_root = tmp.path().to_str().unwrap().to_owned();
    let git = Git::new(Vec::new());

    // A worktree path two levels under a base that does not exist yet.
    let base = tempfile::tempdir().unwrap();
    let missing_base = base.path().join("does/not/exist/yet");
    assert!(
        !missing_base.exists(),
        "the worktree base does not exist before the call"
    );
    let worktree_path = missing_base
        .join("delta-session-1")
        .to_string_lossy()
        .into_owned();

    git.create_worktree(
        &repo_root,
        &worktree_path,
        "delta-session-1",
        WorktreeStartPoint::Head,
    )
    .await
    .expect("the missing parent is created so the worktree add succeeds");

    assert!(
        tokio::fs::metadata(&worktree_path).await.unwrap().is_dir(),
        "the worktree directory exists under the freshly-created base"
    );
}

#[tokio::test]
async fn create_worktree_surfaces_git_stderr_on_failure() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let repo_root = tmp.path().to_str().unwrap().to_owned();
    let git = Git::new(Vec::new());

    let worktree_dir = tempfile::tempdir().unwrap();
    let worktree_path = worktree_dir
        .path()
        .join("wt")
        .to_string_lossy()
        .into_owned();

    // `main` already exists, so `worktree add -b main` must fail; the error
    // surfaces git's stderr rather than a generic message.
    let err = git
        .create_worktree(&repo_root, &worktree_path, "main", WorktreeStartPoint::Head)
        .await
        .unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("git error:"),
        "a worktree-add failure maps to a git error, got: {message}"
    );
}
