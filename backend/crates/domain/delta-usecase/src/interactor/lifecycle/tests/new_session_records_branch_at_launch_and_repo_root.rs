use crate::interactor::testing::*;
use crate::SendTarget;

/// A fresh session in a git-repo workdir captures the spawn-time local branch
/// and the repository root on the session row. The navigator card uses these
/// two values for line 1 (branch) and the basename of line 2 left (repo name);
/// they are a snapshot — never updated on resume or after a later
/// `git checkout` inside the worktree.
#[tokio::test]
async fn new_session_records_branch_at_launch_and_repo_root() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    // The selected directory is a git repository the fake reports a root and a
    // current branch for. The same path is used for both, since the
    // launch dir is the repo root itself in this case.
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, "/projects/app")
        .with_current_branch(&canonical, "feat/widget");
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                workdir: Some("/projects/app".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .unwrap();

    let session = ix
        .store()
        .session(&send.session_id)
        .await
        .unwrap()
        .expect("the eager spawning row was written");
    assert_eq!(
        session.branch_at_launch.as_deref(),
        Some("feat/widget"),
        "the navigator card's line-1 branch is captured at spawn",
    );
    assert_eq!(
        session.repo_root.as_deref(),
        Some("/projects/app"),
        "the navigator card's repo-name source is captured at spawn",
    );
}

/// A fresh session launched outside any git repository records both
/// `branch_at_launch` and `repo_root` as `None`. The frontend then falls back
/// to the cwd basename for the repo-name line and to the session label for
/// the branch line.
#[tokio::test]
async fn new_session_in_a_non_git_dir_records_no_branch_or_repo_root() {
    // No `with_repo` / `with_current_branch` calls: the fake reports
    // "not a git repo" and "no branch", matching the real gateway's behavior
    // outside a repo.
    let ix = interactor();
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/scratch".to_owned());

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                workdir: Some("/scratch".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .unwrap();

    let session = ix
        .store()
        .session(&send.session_id)
        .await
        .unwrap()
        .expect("the eager spawning row was written");
    assert!(
        session.branch_at_launch.is_none(),
        "a non-git launch dir records no branch",
    );
    assert!(
        session.repo_root.is_none(),
        "a non-git launch dir records no repo root",
    );
}
