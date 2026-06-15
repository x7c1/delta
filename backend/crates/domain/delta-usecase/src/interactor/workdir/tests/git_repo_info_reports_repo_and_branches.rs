use crate::interactor::testing::*;

/// `git_repo_info` reports the repo root and default branch for a git directory,
/// and a `None` repo root (and branch) for a non-git directory.
#[tokio::test]
async fn git_repo_info_reports_repo_for_a_git_dir() {
    let git = FakeGitWorktree::default().with_repo("/projects/app", "/projects/app");
    *git.default_branch.lock().unwrap() = Some("main".to_owned());
    let ix = interactor_with_git(git);

    let info = ix.git_repo_info("/projects/app").await.unwrap();
    assert_eq!(info.repo_root.as_deref(), Some("/projects/app"));
    assert_eq!(info.default_branch.as_deref(), Some("main"));

    // A directory the fake knows no repo for reports no root and no branch.
    let plain = ix.git_repo_info("/projects/plain").await.unwrap();
    assert_eq!(plain.repo_root, None);
    assert_eq!(plain.default_branch, None);
}

/// `git_remote_branches` resolves the repo root then fetches its remote
/// branches; a non-git path is a clean `WorktreeNotAGitRepo`.
#[tokio::test]
async fn git_remote_branches_lists_for_a_repo_and_rejects_a_non_repo() {
    let git = FakeGitWorktree::default().with_repo("/projects/app", "/projects/app");
    *git.default_branch.lock().unwrap() = Some("main".to_owned());
    *git.remote_branches.lock().unwrap() = vec!["main".to_owned(), "feature".to_owned()];
    let ix = interactor_with_git(git);

    let remote = ix.git_remote_branches("/projects/app").await.unwrap();
    assert_eq!(remote.default_branch.as_deref(), Some("main"));
    assert_eq!(remote.branches, vec!["main", "feature"]);

    let err = ix.git_remote_branches("/projects/plain").await.unwrap_err();
    assert!(
        matches!(err, crate::error::Error::WorktreeNotAGitRepo(_)),
        "a non-git path is rejected as WorktreeNotAGitRepo, got {err:?}"
    );
}
