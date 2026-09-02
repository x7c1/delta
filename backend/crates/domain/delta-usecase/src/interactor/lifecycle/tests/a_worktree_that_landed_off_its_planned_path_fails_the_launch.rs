use crate::interactor::testing::*;
use crate::ports::{SessionEvent, WorktreeStartPoint};
use crate::{SendTarget, WorktreeSpec};

/// A worktree that lands somewhere other than the path planned at accept time
/// fails the launch instead of starting the agent in a directory that is not
/// there.
///
/// Only a `use_remote_branch` start point can reach this: its plan is "reuse the
/// worktree already holding the branch, else `<base>/<slug>-<id>`", so a second
/// session started from the same PR while the first is still checking out plans
/// the default path (no worktree exists yet) and then finds the first session's
/// worktree at build time. Nothing is created at the planned path — and nothing
/// can be, since git forbids one branch in two worktrees, while the session row
/// already persisted the planned path as its `cwd`. Launching there anyway used
/// to leave the card `Starting` until the bind watchdog reaped it with no
/// reason; now the mismatch is the reason, naming both paths so the user can see
/// where the branch actually is, and a Retry re-plans onto the worktree that now
/// exists.
#[tokio::test]
async fn a_worktree_that_landed_off_its_planned_path_fails_the_launch() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    // Where the *other* session's worktree for `feature/x` already is by the
    // time this launch builds — but not yet when it was planned.
    let elsewhere = "/worktrees/x7c1-delta-earlier-session";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git")
        // Accept phase: not checked out anywhere, so the plan is the default
        // path. Launch task: the branch is now held at `elsewhere`.
        .with_branch_lookups("feature/x", [None, Some(elsewhere)]);
    let (ix, mut sink) = interactor_with_git_and_event_sink(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: Some("/projects/app".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: Some(WorktreeSpec {
                    start_point: WorktreeStartPoint::UseRemoteBranch("feature/x".to_owned()),
                }),
            },
            "hello",
            None,
        )
        .await
        .expect("the send is accepted before the worktree is built");
    let session_id = send.session_id.clone();
    let planned = format!("{TEST_WORKTREE_BASE}/x7c1-delta-{}", session_id.as_str());

    ix.await_launch().await;

    // The failure is announced on the seam, naming the branch and both paths.
    let event = sink.try_recv().expect("a spawn failure was broadcast");
    let SessionEvent::SpawnFailed {
        session_id: failed_id,
        reason,
        ..
    } = event
    else {
        panic!("expected SpawnFailed, got {event:?}");
    };
    assert_eq!(
        failed_id, session_id,
        "the failure names the accepted session"
    );
    let reason = reason.expect("the failure carries a reason the browser can show");
    assert!(
        reason.contains("feature/x"),
        "the reason names the branch that pulled the worktree elsewhere: {reason}"
    );
    assert!(
        reason.contains(&planned),
        "the reason names the planned path, which the row recorded as its cwd: {reason}"
    );
    assert!(
        reason.contains(elsewhere),
        "the reason names where the worktree actually is: {reason}"
    );

    // The row is rolled back, so the session stops being listed…
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the eager row of a failed launch is deleted"
    );
    // …and nothing was launched into the directory that was never created.
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a launch that lost its planned path starts no agent"
    );
    assert!(
        ix.launching_session_ids().await.is_empty(),
        "the launching entry is settled, not left behind"
    );
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "a failed launch never becomes a pending spawn"
    );
}
