//! What git reports about a directory: its repository root, its `origin` URL
//! and its current branch.

use super::*;

#[tokio::test]
async fn repo_root_reports_the_toplevel_for_a_git_dir() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let git = Git::new(Vec::new());

    let root = git
        .repo_root(tmp.path().to_str().unwrap())
        .await
        .unwrap()
        .expect("a git repo reports a root");

    // Compare canonicalized: git may print a symlink-resolved path (macOS
    // `/var` → `/private/var`).
    let expected = tokio::fs::canonicalize(tmp.path())
        .await
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let actual = tokio::fs::canonicalize(&root)
        .await
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn origin_url_reports_the_configured_remote_url() {
    // A fresh init has no origin remote; setting one makes `origin_url`
    // report it. The value is returned verbatim, with any trailing newline
    // trimmed.
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let repo = tmp.path().to_str().unwrap();
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/x7c1/delta.git",
        ])
        .output()
        .await
        .unwrap();
    let git = Git::new(Vec::new());
    let url = git
        .origin_url(repo)
        .await
        .unwrap()
        .expect("origin url is configured");
    assert_eq!(url, "https://github.com/x7c1/delta.git");
}

#[tokio::test]
async fn origin_url_is_none_when_remote_is_unset() {
    // A fresh init has no `remote.origin.url`; `git config --get` exits
    // non-zero, which the gateway translates to `None`.
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let git = Git::new(Vec::new());
    let url = git.origin_url(tmp.path().to_str().unwrap()).await.unwrap();
    assert!(url.is_none(), "no origin remote means no origin url");
}

#[tokio::test]
async fn origin_url_is_none_outside_a_git_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let git = Git::new(Vec::new());
    let url = git.origin_url(tmp.path().to_str().unwrap()).await.unwrap();
    assert!(
        url.is_none(),
        "a non-git directory has no origin url to report"
    );
}

#[tokio::test]
async fn current_branch_reports_the_local_branch_name() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let git = Git::new(Vec::new());

    let branch = git
        .current_branch(tmp.path().to_str().unwrap())
        .await
        .unwrap()
        .expect("a fresh repo on its initial branch reports a branch");
    assert_eq!(branch, "main");
}

#[tokio::test]
async fn current_branch_is_none_outside_a_git_repo() {
    // A bare temp directory with no git repo above it.
    let tmp = tempfile::tempdir().unwrap();
    let git = Git::new(Vec::new());

    let branch = git
        .current_branch(tmp.path().to_str().unwrap())
        .await
        .unwrap();
    assert!(branch.is_none(), "a non-git directory has no branch");
}

#[tokio::test]
async fn current_branch_is_none_when_head_is_detached() {
    // A detached HEAD has no branch name to record at launch time;
    // `rev-parse --abbrev-ref HEAD` returns the literal `HEAD`, which the
    // gateway translates to `None`.
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path()).await;
    let repo = tmp.path().to_str().unwrap();
    // Detach HEAD onto the initial commit so HEAD no longer points at a
    // branch ref.
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["checkout", "--detach", "HEAD"])
        .output()
        .await
        .unwrap();
    assert!(
        status.status.success(),
        "detach failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let git = Git::new(Vec::new());

    let branch = git.current_branch(repo).await.unwrap();
    assert!(branch.is_none(), "a detached HEAD reports no branch");
}

#[tokio::test]
async fn repo_root_is_none_outside_a_git_repo() {
    // A bare temp directory with no git repo above it.
    let tmp = tempfile::tempdir().unwrap();
    let git = Git::new(Vec::new());

    let root = git.repo_root(tmp.path().to_str().unwrap()).await.unwrap();
    assert!(root.is_none(), "a non-git directory has no repo root");
}
