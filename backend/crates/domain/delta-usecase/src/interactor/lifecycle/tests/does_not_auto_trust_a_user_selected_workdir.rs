use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::SendTarget;

/// A fresh session whose user-selected workdir is a git repository (no worktree
/// request) does NOT pre-accept the workspace-trust dialog for that directory:
/// it sits outside Delta's own worktree base, so Delta leaves it to Claude
/// Code's normal one-time dialog rather than silently trusting the repo's
/// checked-in automation in the user's plain `claude` sessions.
#[tokio::test]
async fn does_not_auto_trust_a_user_selected_workdir() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    // The selected directory is a git repository the fake reports a root for,
    // but it is not under `TEST_WORKTREE_BASE`.
    let git = FakeGitWorktree::default().with_repo(&canonical, "/projects/app");
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    ix.enqueue_send(
        SendTarget::NewSession {
            pull_request_number: None,
            provider: crate::AgentProvider::Claude,
            workdir: Some("/projects/app".to_owned()),
            launch_option_ids: Vec::new(),
            worktree: None,
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    ix.await_launch().await;

    assert!(
        ix.git_worktree_fake().trusted.lock().unwrap().is_empty(),
        "a user-selected git repo outside the worktree base is not auto-trusted"
    );
}

/// Resuming a session whose stored cwd is a git repository the user selected
/// (outside Delta's worktree base) likewise does NOT pre-accept the trust
/// dialog — the same scoping applies on the resume path.
#[tokio::test]
async fn resume_does_not_auto_trust_a_user_selected_workdir() {
    // The session's cwd is a git repo, but it is not under `TEST_WORKTREE_BASE`.
    let git = FakeGitWorktree::default().with_repo("/projects/app", "/projects/app");
    let ix = interactor_with_git(git);
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/projects/app/t.jsonl",
        "/projects/app",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");

    ix.open_session(&id).await.unwrap();

    assert!(
        ix.git_worktree_fake().trusted.lock().unwrap().is_empty(),
        "a user-selected git-repo cwd outside the worktree base is not auto-trusted on resume"
    );
}
