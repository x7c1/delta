use std::time::Duration;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, WorktreeStartPoint};
use crate::{LaunchConfig, SendTarget, WorktreeSpec};

/// A launch preparation that never finishes is given up on, and says so.
///
/// The sequence is unbounded from Delta's side — a `git fetch` can hang on an
/// unreachable remote or a credential prompt with no timeout of its own — and
/// nothing else watches an accepted session: the bind watchdog only starts once
/// a pane exists. So the preparation carries its own deadline
/// ([`LaunchConfig::launch_prep_deadline`], ten minutes in production, shrunk to
/// milliseconds here), and reaching it rolls the acceptance back exactly as a
/// failed build does. The worktree gate stays shut for the whole test: the point
/// is that the deadline wins over a build that never returns.
#[tokio::test]
async fn a_launch_preparation_that_outruns_its_deadline_reports_spawn_failed() {
    let gate = WorktreeGate::closed();
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git")
        .with_gate(&gate);
    let (ix, mut sink) = interactor_with_git_and_event_sink(git);
    let ix = ix.with_launch_config(LaunchConfig {
        launch_prep_deadline: Duration::from_millis(50),
        ..LaunchConfig::default()
    });
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
                    start_point: WorktreeStartPoint::Head,
                }),
            },
            "hello",
            None,
        )
        .await
        .expect("the send is accepted while the worktree build is still held");
    let session_id = send.session_id.clone();

    // The build is still parked on the gate; the deadline settles the launch.
    ix.await_launch().await;

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
        reason.contains("launch preparation timed out"),
        "the reason is the timeout, not some other launch failure: {reason}"
    );
    assert!(
        reason.contains(&format!(
            "{TEST_WORKTREE_BASE}/x7c1-delta-{}",
            session_id.as_str()
        )),
        "the reason names the launch that got stuck: {reason}"
    );

    // Rolled back exactly as a failed build is.
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the eager row of an abandoned launch is deleted"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a preparation that never finished launches no agent"
    );
    assert!(
        ix.launching_session_ids().await.is_empty(),
        "the launching entry is settled, not left behind"
    );
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "an abandoned launch never becomes a pending spawn"
    );
}
