use crate::interactor::testing::*;
use crate::SendTarget;

/// A fresh session whose user-selected workdir is a git repository (no worktree
/// request) pre-accepts the workspace-trust dialog for that directory, so
/// launching `claude` in a real repo never stalls on the trust dialog.
#[tokio::test]
async fn new_session_with_git_repo_workdir_seeds_trust() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    // The selected directory is a git repository the fake reports a root for.
    let git = FakeGitWorktree::default().with_repo(&canonical, "/projects/app");
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    ix.enqueue_send(
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

    let trusted = ix.git_worktree_fake().trusted.lock().unwrap().clone();
    assert_eq!(
        trusted,
        vec![canonical],
        "trust is seeded for the selected git-repo workdir"
    );
}
