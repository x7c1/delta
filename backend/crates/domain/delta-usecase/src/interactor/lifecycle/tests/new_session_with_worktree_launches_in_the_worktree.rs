use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A composer-first send that opts into a worktree, when the selected workdir is
/// a git repository, creates a per-session worktree at `<base>/delta-<id>` and
/// launches there — not in the selected directory itself — and that worktree
/// path is both the tmux launch dir and the stored session cwd.
#[tokio::test]
async fn new_session_with_worktree_launches_in_the_worktree() {
    // The selected directory resolves (FakeWorkspace canonicalizes it) and is a
    // git repository whose root the fake reports.
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

    // The minted session id is the worktree path/branch suffix.
    let session_id = ix.pending_session_ids().await.remove(0);
    let expected_path = format!("{TEST_WORKDIR_BASE}/delta-{}", session_id.as_str());

    // A worktree was created off HEAD at the repo root the detection reported,
    // on a `delta-<id>` branch, at the per-session path.
    let created_worktrees = ix.git_worktree_fake().created.lock().unwrap().clone();
    assert_eq!(created_worktrees.len(), 1, "one worktree created");
    let wt = &created_worktrees[0];
    assert_eq!(wt.repo_root, "/projects/app/.git/..");
    assert_eq!(wt.worktree_path, expected_path);
    assert_eq!(wt.branch, format!("delta-{}", session_id.as_str()));
    assert_eq!(wt.start_point, WorktreeStartPoint::Head);

    // The pane launches in the worktree, not the selected directory.
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session spawned");
    assert_eq!(
        created[0].workdir, expected_path,
        "the launch dir is the worktree path"
    );

    // The eager session row stored the worktree path as its cwd, so a later
    // resume reattaches to the existing worktree.
    let stored = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the eager session row exists");
    assert_eq!(stored.cwd, expected_path, "stored cwd is the worktree path");
}
