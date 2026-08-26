use std::time::Instant;

use crate::interactor::session_actor::runtime::PENDING_SPAWN_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// The launch watchdog leaves an in-flight launch alone, and starts its clock
/// when the launch actually comes up.
///
/// The bind deadline exists for one thing: a pane that came up and never fired
/// its first hook. A session whose worktree is still being checked out has no
/// pane at all, so however long that takes it must not be reaped — otherwise a
/// slow `git fetch` would eat the whole deadline and a healthy session would be
/// killed seconds after its agent finally started. So the watchdog ignores a
/// launching session entirely, and the spawn it hands over is stamped where the
/// launch records it — just before the pane is created — rather than at
/// acceptance.
#[tokio::test]
async fn the_bind_deadline_starts_at_the_launch_not_at_acceptance() {
    let gate = WorktreeGate::closed();
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_gate(&gate);
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let accepted_at = Instant::now();
    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
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
    let session_id = send.session_id.clone();

    // A build that has been running for ten times the bind deadline is still
    // not the watchdog's business: nothing is reaped and the row survives.
    let events = ix
        .reap_stale_spawns(accepted_at + PENDING_SPAWN_DEADLINE * 10)
        .await
        .unwrap();
    assert!(
        events.is_empty(),
        "a launch that is still being prepared is never reaped"
    );
    assert!(
        ix.store().session(&session_id).await.unwrap().is_some(),
        "the accepted row survives a long preparation"
    );

    gate.open();
    ix.await_launch().await;

    // The deadline now runs from the launch: a sweep at the moment it came up
    // leaves it alone…
    let launched_at = Instant::now();
    let events = ix.reap_stale_spawns(launched_at).await.unwrap();
    assert!(
        events.is_empty(),
        "the freshly-launched spawn gets its full deadline"
    );
    assert_eq!(ix.pending_session_ids().await, vec![session_id.clone()]);

    // …and only a sweep a full deadline past the launch reaps it.
    let events = ix
        .reap_stale_spawns(launched_at + PENDING_SPAWN_DEADLINE)
        .await
        .unwrap();
    assert_eq!(
        events.len(),
        1,
        "a spawn that never binds is still reaped, measured from the launch"
    );
}
