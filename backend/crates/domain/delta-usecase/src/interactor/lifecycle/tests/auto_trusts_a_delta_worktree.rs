use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A fresh session whose worktree Delta creates under its own worktree base
/// pre-accepts Claude Code's workspace-trust dialog for that worktree path — a
/// directory Delta itself made, so seeding it never silently trusts a repo the
/// user pointed Delta at. The interactive launch there is not blocked on the
/// trust dialog.
#[tokio::test]
async fn auto_trusts_a_delta_worktree() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git");
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    ix.enqueue_send(
        SendTarget::NewSession {
            provider: crate::AgentProvider::Claude,
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
    // The worktree Delta builds lands under its own worktree base.
    let expected_path = format!("{TEST_WORKTREE_BASE}/x7c1-delta-{}", session_id.as_str());

    let trusted = ix.git_worktree_fake().trusted.lock().unwrap().clone();
    assert_eq!(
        trusted,
        vec![expected_path],
        "trust is seeded for the Delta worktree under the worktree base"
    );
}
