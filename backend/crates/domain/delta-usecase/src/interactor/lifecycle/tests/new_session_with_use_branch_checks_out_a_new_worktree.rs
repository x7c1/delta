use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A composer-first send that opts into the "use this branch" mode, when the
/// chosen branch is *not* checked out anywhere, creates a worktree that checks
/// the branch out (via `add_worktree_checkout`, not a new `delta-<id>` branch)
/// at `<worktree_base>/delta-<id>`, launches there, and seeds trust for it.
#[tokio::test]
async fn new_session_with_use_branch_checks_out_a_new_worktree() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    // `feature` is not checked out anywhere (absent from the scripted map).
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
                start_point: WorktreeStartPoint::UseRemoteBranch("feature".to_owned()),
            }),
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    let session_id = ix.pending_session_ids().await.remove(0);
    let expected_path = format!("{TEST_WORKTREE_BASE}/delta-{}", session_id.as_str());

    // A checkout worktree was added for the branch at the per-session path; no
    // new-branch worktree was created.
    assert!(
        ix.git_worktree_fake().created.lock().unwrap().is_empty(),
        "no new-branch worktree is created in the use-branch mode"
    );
    let checked_out = ix.git_worktree_fake().checked_out.lock().unwrap().clone();
    assert_eq!(checked_out.len(), 1, "one checkout worktree added");
    assert_eq!(checked_out[0].repo_root, "/projects/app/.git/..");
    assert_eq!(checked_out[0].worktree_path, expected_path);
    assert_eq!(checked_out[0].branch, "feature");

    // The pane launches in the new worktree.
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session spawned");
    assert_eq!(
        created[0].workdir, expected_path,
        "the launch dir is the new worktree path"
    );

    // Trust seeded for the new worktree path; stored cwd matches.
    let trusted = ix.git_worktree_fake().trusted.lock().unwrap().clone();
    assert_eq!(trusted, vec![expected_path.clone()]);
    let stored = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the eager session row exists");
    assert_eq!(stored.cwd, expected_path);
}
