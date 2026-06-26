use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A composer-first send that opts into a worktree, when the selected workdir is
/// a git repository, creates a per-session worktree at
/// `<worktree_base>/<org>-<repo>-<id>` (the neutral base outside any repo tree,
/// *not* `session_workdir_base`) and launches there — not in the selected
/// directory itself — and that worktree path is both the tmux launch dir and
/// the stored session cwd.
#[tokio::test]
async fn new_session_with_worktree_launches_in_the_worktree() {
    // The selected directory resolves (FakeWorkspace canonicalizes it) and is a
    // git repository whose root the fake reports.
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

    // The minted session id suffixes the worktree path; the path also embeds
    // the `<org>-<repo>` slug derived from the origin URL. The worktree lives
    // under the neutral `worktree_base`, not `session_workdir_base`.
    let session_id = ix.pending_session_ids().await.remove(0);
    let expected_path = format!("{TEST_WORKTREE_BASE}/x7c1-delta-{}", session_id.as_str());
    assert!(
        !expected_path.starts_with(&format!("{TEST_WORKDIR_BASE}/")),
        "the worktree lives under the neutral worktree base, not the session workdir base"
    );

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
    // resume reattaches to the existing worktree. `requested_workdir` keeps
    // the user-selected dir (here the canonicalized `/projects/app`) so the
    // Recent dirs picker surfaces that instead of the worktree path.
    let stored = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the eager session row exists");
    assert_eq!(stored.cwd, expected_path, "stored cwd is the worktree path");
    assert_eq!(
        stored.requested_workdir.as_deref(),
        Some(canonical.as_str()),
        "requested_workdir keeps the user-selected dir, not the worktree path",
    );
}
