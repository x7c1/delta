use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A composer-first send that opts into a worktree with the "use this branch"
/// mode, when the chosen branch is already checked out somewhere (here the main
/// working tree), launches in that *existing* worktree directly: no new worktree
/// is created (neither `add_worktree_checkout` nor `create_worktree` runs), the
/// existing path is the tmux launch dir and the stored session cwd, and trust is
/// seeded for it (idempotent, so reusing an already-trusted path is fine).
#[tokio::test]
async fn new_session_with_use_branch_reuses_an_existing_worktree() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    // `main` is already checked out at the repo root (the main working tree).
    let existing = "/projects/app/.git/..".to_owned();
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, "/projects/app/.git/..")
        .with_branch_checked_out("main", &existing);
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
                start_point: WorktreeStartPoint::UseRemoteBranch("main".to_owned()),
            }),
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    let session_id = ix.pending_session_ids().await.remove(0);

    // No worktree was created: the existing one is reused.
    assert!(
        ix.git_worktree_fake().created.lock().unwrap().is_empty(),
        "no new-branch worktree is created when the branch is already checked out"
    );
    assert!(
        ix.git_worktree_fake()
            .checked_out
            .lock()
            .unwrap()
            .is_empty(),
        "no checkout worktree is added when the branch is already checked out"
    );

    // The pane launches in the existing worktree, not a `delta-<id>` path.
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session spawned");
    assert_eq!(
        created[0].workdir, existing,
        "the launch dir is the existing worktree path"
    );

    // Trust seeded for the existing path.
    let trusted = ix.git_worktree_fake().trusted.lock().unwrap().clone();
    assert_eq!(
        trusted,
        vec![existing.clone()],
        "trust seeded for the reused path"
    );

    // The eager session row stored the existing worktree path as its cwd.
    let stored = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the eager session row exists");
    assert_eq!(
        stored.cwd, existing,
        "stored cwd is the existing worktree path"
    );
}
