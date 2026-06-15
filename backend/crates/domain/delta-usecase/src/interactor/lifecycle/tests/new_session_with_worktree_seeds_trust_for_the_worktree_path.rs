use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A fresh session that creates a worktree pre-accepts Claude Code's
/// workspace-trust dialog for the worktree path (a git working tree), so the
/// interactive launch there is not blocked on the trust dialog.
#[tokio::test]
async fn new_session_with_worktree_seeds_trust_for_the_worktree_path() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let git = FakeGitWorktree::default().with_repo(&canonical, "/projects/app/.git/..");
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
            worktree: Some(WorktreeSpec {
                start_point: WorktreeStartPoint::Head,
            }),
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    let session_id = ix.pending_session_ids().await.remove(0);
    let expected_path = format!("{TEST_WORKDIR_BASE}/delta-{}", session_id.as_str());

    let trusted = ix.git_worktree_fake().trusted.lock().unwrap().clone();
    assert_eq!(
        trusted,
        vec![expected_path],
        "trust is seeded for the worktree path"
    );
}
