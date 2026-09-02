use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A worktree requested without a selected workdir is rejected: a worktree needs
/// a git repository to branch off, so with no directory the send returns
/// `WorktreeRequiresWorkdir` and spawns nothing.
#[tokio::test]
async fn new_session_with_worktree_but_no_workdir_is_rejected() {
    let ix = interactor_with_git(FakeGitWorktree::default());

    let err = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: None,
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
        matches!(err, crate::error::Error::WorktreeRequiresWorkdir),
        "a worktree without a workdir is rejected as WorktreeRequiresWorkdir, got {err:?}"
    );
    assert!(
        ix.git_worktree_fake().created.lock().unwrap().is_empty(),
        "no worktree is created"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no pane is created"
    );
}
