use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A worktree requested for a directory that resolves but is not a git
/// repository is rejected before anything is spawned: the send returns
/// `WorktreeNotAGitRepo`, no worktree is created, no pane is launched, and no
/// pending spawn is left behind.
#[tokio::test]
async fn new_session_with_worktree_on_a_non_git_dir_is_rejected() {
    // The directory exists (resolves) but the git fake knows of no repo there.
    let ix = interactor_with_git(FakeGitWorktree::default());
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/plain".to_owned());

    let err = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: Some("/projects/plain".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: Some(WorktreeSpec {
                    start_point: WorktreeStartPoint::Head,
                }),
            },
            "hello",
            None,
        )
        .await
        .unwrap_err();

    assert!(
        matches!(err, crate::error::Error::WorktreeNotAGitRepo(_)),
        "a worktree on a non-git dir is rejected as WorktreeNotAGitRepo, got {err:?}"
    );
    assert!(
        ix.git_worktree_fake().created.lock().unwrap().is_empty(),
        "no worktree is created when the dir is not a git repo"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no pane is created"
    );
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "no pending spawn is recorded"
    );
}
