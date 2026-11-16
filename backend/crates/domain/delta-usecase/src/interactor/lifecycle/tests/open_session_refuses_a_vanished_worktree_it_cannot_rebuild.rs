use delta_model::SessionId;

use crate::error::Error;
use crate::interactor::testing::*;

use super::open_session_rebuilds_a_vanished_worktree_at_its_path::closed_session_with_snapshot;

/// The closed session's vanished working directory.
const GONE: &str = "/elsewhere/gone";
/// The repository the session records.
const REPO: &str = "/repos/delta";
/// The branch the session records.
const BRANCH: &str = "feature/resume";

/// Resume a closed session in [`GONE`] whose snapshot records `repo_root` and
/// `branch_at_launch`, against `git`, and assert it is refused with
/// [`Error::WorktreeUnrecoverable`] naming the directory and `reason` — with no
/// worktree, no pane and no settings created, and the session left closed.
async fn assert_refused(
    git: FakeGitWorktree,
    repo_root: Option<&str>,
    branch_at_launch: Option<&str>,
    reason: &str,
) {
    let ix =
        interactor_with_workspace_and_git(FakeWorkspace::default().with_missing_dir(GONE), git);
    let id = SessionId::from("sess-G");
    closed_session_with_snapshot(&ix, &id, GONE, repo_root, branch_at_launch).await;

    let err = ix
        .open_session(&id)
        .await
        .expect_err("a session that names no branch is not resumed");
    assert!(
        matches!(err, Error::WorktreeUnrecoverable { ref dir, .. } if dir == GONE),
        "the vanished directory is named, got: {err:?}"
    );
    let message = err.to_string();
    assert!(
        message.contains(GONE) && message.contains(reason),
        "the message names the directory and why ({reason:?}): {message}"
    );

    assert!(
        ix.git_worktree_fake()
            .checked_out
            .lock()
            .unwrap()
            .is_empty(),
        "no worktree is checked out"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no pane is created"
    );
    assert!(
        ix.workspace_fake().written.lock().unwrap().is_empty(),
        "no session settings are written"
    );
    assert!(
        ix.bound_pane(&id).await.is_none(),
        "the session stays closed"
    );
}

#[tokio::test]
async fn a_session_that_recorded_no_branch_is_refused() {
    assert_refused(
        FakeGitWorktree::default().with_repo(REPO, REPO),
        Some(REPO),
        None,
        "no branch at launch",
    )
    .await;
}

#[tokio::test]
async fn a_session_that_recorded_no_repository_is_refused() {
    assert_refused(
        FakeGitWorktree::default(),
        None,
        Some(BRANCH),
        "no repository at launch",
    )
    .await;
}

/// The recorded repository is gone (or no longer a git repository): the fake
/// reports it as no repository at all.
#[tokio::test]
async fn a_session_whose_repository_is_gone_is_refused() {
    assert_refused(
        FakeGitWorktree::default(),
        Some(REPO),
        Some(BRANCH),
        "no longer exists or is not a git repository",
    )
    .await;
}

#[tokio::test]
async fn a_session_whose_branch_exists_nowhere_is_refused() {
    assert_refused(
        FakeGitWorktree::default()
            .with_repo(REPO, REPO)
            .with_absent_branch(BRANCH),
        Some(REPO),
        Some(BRANCH),
        "exists neither locally nor on origin",
    )
    .await;
}

/// A recorded branch name git could read as an option never reaches git.
#[tokio::test]
async fn a_session_whose_branch_name_is_flag_shaped_is_refused() {
    assert_refused(
        FakeGitWorktree::default().with_repo(REPO, REPO),
        Some(REPO),
        Some("--upload-pack=/tmp/x"),
        "not a valid branch name",
    )
    .await;
}
