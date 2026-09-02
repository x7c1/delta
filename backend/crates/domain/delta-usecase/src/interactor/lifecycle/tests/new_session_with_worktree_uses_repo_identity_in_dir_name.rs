use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// The per-session worktree directory name embeds the repository identity:
/// when the launch directory's `origin` URL resolves to `host/x7c1/delta`,
/// the worktree path is `<base>/x7c1-delta-<session-id>` (not the legacy
/// `<base>/delta-<session-id>`). The git **branch** created for new-branch
/// start points stays `delta-<session-id>` so the frontend's `displayBranch()`
/// shortening continues to match it.
#[tokio::test]
async fn new_session_with_worktree_uses_repo_identity_in_dir_name() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta");
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
    let expected_path = format!("{TEST_WORKTREE_BASE}/x7c1-delta-{}", session_id.as_str());
    let expected_branch = format!("delta-{}", session_id.as_str());

    let created = ix.git_worktree_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one worktree created");
    assert_eq!(
        created[0].worktree_path, expected_path,
        "worktree path embeds the `<org>-<repo>` slug derived from origin",
    );
    assert_eq!(
        created[0].branch, expected_branch,
        "branch name keeps the legacy `delta-<session-id>` shape so displayBranch() still matches",
    );
}
