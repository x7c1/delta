//! Shared setup for the launch-trust tests: a repository the user selected,
//! a worktree launch from it, and what the launch did about trust.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{AsyncEventReceiver, SessionEvent, WorktreeStartPoint};
use crate::{LaunchTrustCheck, SendTarget, WorktreeSpec};

/// The directory the user picks.
pub(super) const WORKDIR: &str = "/projects/app";

/// The repository root the fake reports for [`WORKDIR`] — outside
/// [`TEST_WORKTREE_BASE`], as a user's repository is.
pub(super) const REPO_ROOT: &str = "/projects/app/.git/..";

/// A fake git that knows [`WORKDIR`] as a repository rooted at [`REPO_ROOT`],
/// which Claude Code's config trusts unless the caller says otherwise.
pub(super) fn repository() -> FakeGitWorktree {
    FakeGitWorktree::default()
        .with_repo(&FakeWorkspace::canonical(WORKDIR), REPO_ROOT)
        .with_origin_url(REPO_ROOT, "https://github.com/x7c1/delta.git")
}

/// An interactor over `git` whose workspace has [`WORKDIR`], with the async
/// event seam the launch's failures arrive on.
pub(super) fn interactor_over(git: FakeGitWorktree) -> (TestInteractor, AsyncEventReceiver) {
    let (ix, sink) = interactor_with_git_and_event_sink(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push(WORKDIR.to_owned());
    (ix, sink)
}

/// What the new-session flow asks before a Claude launch from `start_point`.
pub(super) async fn check(
    ix: &TestInteractor,
    start_point: &WorktreeStartPoint,
) -> LaunchTrustCheck {
    ix.check_launch_trust(WORKDIR, start_point, crate::AgentProvider::Claude)
        .await
        .expect("the launch-trust check succeeds")
}

/// Send a new Claude session in a worktree started from `start_point`,
/// carrying `confirmed_commit`, and wait for its launch to finish. Returns the
/// session's id and the worktree path it planned.
pub(super) async fn launch(
    ix: &TestInteractor,
    start_point: WorktreeStartPoint,
    confirmed_commit: Option<&str>,
) -> (SessionId, String) {
    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: Some(WORKDIR.to_owned()),
                launch_option_ids: Vec::new(),
                worktree: Some(WorktreeSpec {
                    start_point,
                    confirmed_commit: confirmed_commit.map(str::to_owned),
                }),
            },
            "hello",
            None,
        )
        .await
        .expect("the send is accepted before the launch runs");
    ix.await_launch().await;
    let path = format!(
        "{TEST_WORKTREE_BASE}/x7c1-delta-{}",
        send.session_id.as_str()
    );
    (send.session_id, path)
}

/// The dirs Delta pre-accepted (all keys) so far.
pub(super) fn pre_accepted(ix: &TestInteractor) -> Vec<String> {
    ix.git_worktree_fake().trusted.lock().unwrap().clone()
}

/// The repository roots Delta recorded trust for so far.
pub(super) fn recorded_repositories(ix: &TestInteractor) -> Vec<String> {
    ix.git_worktree_fake()
        .recorded_trust
        .lock()
        .unwrap()
        .clone()
}

/// Assert the launch was stopped for want of a confirmation: a
/// `spawn_failed` saying so, no agent started, nothing pre-accepted.
pub(super) fn assert_stopped_for_confirmation(ix: &TestInteractor, sink: &mut AsyncEventReceiver) {
    let event = sink.try_recv().expect("the stopped launch is announced");
    let SessionEvent::SpawnFailed { reason, .. } = event else {
        panic!("expected SpawnFailed, got {event:?}");
    };
    assert!(
        reason
            .as_deref()
            .is_some_and(|r| r.contains("did not confirm")),
        "the failure says the commit was not confirmed, got {reason:?}"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no agent starts in a worktree whose trust was not pre-accepted"
    );
    assert!(pre_accepted(ix).is_empty(), "nothing is pre-accepted");
    assert!(
        recorded_repositories(ix).is_empty(),
        "no repository trust is recorded"
    );
}
