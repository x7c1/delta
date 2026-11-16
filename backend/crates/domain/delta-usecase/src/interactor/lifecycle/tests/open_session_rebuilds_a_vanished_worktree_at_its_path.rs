use delta_model::SessionId;

use crate::error::Error;
use crate::interactor::testing::*;
use crate::ports::NewSession;

/// The worktree a closed session was launched in, under the worktree base.
const WORKTREE: &str = "/tmp/delta-worktrees/x7c1-delta-sess-W";
/// The repository that worktree was cut from.
const REPO: &str = "/repos/delta";
/// The branch the session was launched on.
const BRANCH: &str = "feature/resume";

/// Register `id` as a closed session in `cwd` whose spawn-time snapshot
/// records `repo_root` and `branch_at_launch`, as a launch from a pull request
/// does, and whose transcript is known.
pub(super) async fn closed_session_with_snapshot(
    ix: &TestInteractor,
    id: &SessionId,
    cwd: &str,
    repo_root: Option<&str>,
    branch_at_launch: Option<&str>,
) {
    ix.store()
        .register_session(NewSession {
            id: id.clone(),
            cwd: cwd.to_owned(),
            transcript_path: format!("{cwd}/t.jsonl"),
            branch_at_launch: branch_at_launch.map(str::to_owned),
            repo_root: repo_root.map(str::to_owned),
            repository_display_name: None,
        })
        .await
        .unwrap();
    assert!(ix.bound_pane(id).await.is_none(), "starts closed");
}

/// The interactor for a session whose worktree at [`WORKTREE`] is gone, with
/// [`REPO`] a git repository; a successful checkout makes the directory exist
/// again.
fn interactor_with_vanished_worktree(git: FakeGitWorktree) -> TestInteractor {
    let workspace = FakeWorkspace::default().with_missing_dir(WORKTREE);
    let git = git
        .with_repo(REPO, REPO)
        .with_checkouts_creating_dirs_in(workspace.missing_dirs_handle());
    interactor_with_workspace_and_git(workspace, git)
}

/// Resuming a closed session whose worktree directory is gone checks the
/// session's branch out in a new worktree of its repository at the original
/// path, then resumes there exactly as it would have: `claude --resume <id>` in
/// that directory.
#[tokio::test]
async fn open_session_rebuilds_a_vanished_worktree_at_its_path() {
    let ix = interactor_with_vanished_worktree(FakeGitWorktree::default());
    let id = SessionId::from("sess-W");
    closed_session_with_snapshot(&ix, &id, WORKTREE, Some(REPO), Some(BRANCH)).await;

    ix.open_session(&id).await.unwrap();

    let checked_out = ix.git_worktree_fake().checked_out.lock().unwrap().clone();
    assert_eq!(checked_out.len(), 1, "one worktree is checked out");
    assert_eq!(checked_out[0].repo_root, REPO);
    assert_eq!(
        checked_out[0].worktree_path, WORKTREE,
        "the worktree is rebuilt at the session's original cwd"
    );
    assert_eq!(checked_out[0].branch, BRANCH);
    assert_eq!(
        *ix.git_worktree_fake().restored.lock().unwrap(),
        vec![WORKTREE.to_owned()],
        "the session's branch is restored, not fetched as a pull-request launch is"
    );

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one pane is spawned");
    assert_eq!(
        created[0].workdir, WORKTREE,
        "the pane runs in the rebuilt dir"
    );
    assert!(
        created[0]
            .command
            .windows(2)
            .any(|pair| pair[0] == "--resume" && pair[1] == "sess-W"),
        "the pane resumes the session: {:?}",
        created[0].command
    );
    assert!(ix.bound_pane(&id).await.is_some(), "the session is open");
}

/// When git refuses the checkout (the branch is checked out in another
/// worktree that still exists, say), the resume fails with git's error: no
/// pane is created and the session stays closed.
#[tokio::test]
async fn open_session_fails_with_gits_error_when_the_rebuild_is_refused() {
    let ix =
        interactor_with_vanished_worktree(FakeGitWorktree::default().with_failing_worktree_add());
    let id = SessionId::from("sess-W");
    closed_session_with_snapshot(&ix, &id, WORKTREE, Some(REPO), Some(BRANCH)).await;

    let err = ix
        .open_session(&id)
        .await
        .expect_err("a refused checkout fails the resume");
    assert!(
        matches!(err, Error::Git(ref message) if message.contains("worktree add failed")),
        "git's own error is returned, got: {err:?}"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no pane is created"
    );
    assert!(
        ix.workspace_fake().written.lock().unwrap().is_empty(),
        "no session settings are written for a failed resume"
    );
    assert!(
        ix.bound_pane(&id).await.is_none(),
        "the session stays closed"
    );
}

/// A closed session whose cwd still exists resumes there without touching
/// git: no branch is looked up and no checkout is requested.
#[tokio::test]
async fn open_session_in_an_existing_cwd_requests_no_checkout() {
    let ix = interactor_with_git(FakeGitWorktree::default().with_repo(REPO, REPO));
    let id = SessionId::from("sess-W");
    closed_session_with_snapshot(&ix, &id, WORKTREE, Some(REPO), Some(BRANCH)).await;

    ix.open_session(&id).await.unwrap();

    let git = ix.git_worktree_fake();
    assert!(
        git.checked_out.lock().unwrap().is_empty(),
        "no checkout is requested for an existing cwd"
    );
    assert!(
        git.restored.lock().unwrap().is_empty(),
        "no restore is requested for an existing cwd"
    );
    assert!(
        git.has_branch_calls.lock().unwrap().is_empty(),
        "no branch is looked up for an existing cwd"
    );
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "the session resumes");
    assert_eq!(created[0].workdir, WORKTREE);
}
