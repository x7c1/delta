use crate::interactor::testing::*;
use crate::ports::{SessionEvent, WorktreeStartPoint};
use crate::{SendTarget, WorktreeSpec};

/// Closing a session while its launch preparation is still running cancels the
/// launch, and the preparation is then abandoned rather than completed.
///
/// This is the shape a wedged launch is stuck in for real: a `git fetch` that
/// hangs has no pane to kill and no pending spawn to reap, so before this the
/// card sat amber with nothing the user could do. Taking the launching entry is
/// the whole cancellation — the background task keeps running, but its
/// checkpoint now finds no entry, answers `Abandon`, and never creates a pane
/// nothing could bind or kill.
#[tokio::test]
async fn close_session_cancels_a_launch_still_preparing() {
    let gate = WorktreeGate::closed();
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git")
        .with_gate(&gate);
    let ix = interactor_with_git(git);
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
            "start something",
            None,
        )
        .await
        .expect("the send is accepted while the worktree build is still held");
    let session_id = send.session_id.clone();
    assert_eq!(
        ix.launching_session_ids().await,
        vec![session_id.clone()],
        "the launch is parked on the gate, with no pane yet"
    );

    // Close it while the build is parked — the wedged-launch case.
    let events = ix.close_session(&session_id).await.unwrap();

    let [SessionEvent::SpawnFailed {
        session_id: failed_id,
        pane_token,
        reason,
        cancelled,
        unsent,
    }] = events.as_slice()
    else {
        panic!("expected exactly one SpawnFailed, got {events:?}");
    };
    assert_eq!(
        failed_id, &session_id,
        "the failure names the closed session"
    );
    assert_eq!(
        pane_token.as_deref(),
        Some("delta-1"),
        "the launch's minted pane token travels on the event"
    );
    assert_eq!(reason.as_deref(), Some("closed while starting"));
    assert!(
        *cancelled,
        "the user asked for this, so the browser words it as a cancel"
    );
    assert_eq!(
        unsent.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        vec!["start something"],
        "the undelivered first prompt is handed back to the composer"
    );
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the eager row of a cancelled launch is deleted"
    );
    assert!(
        ix.launching_session_ids().await.is_empty(),
        "the launching entry is taken by the close"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "a preparation that never reached tmux leaves no pane to reclaim"
    );

    // Let the held preparation run to completion: its checkpoint must find
    // nothing left to launch.
    gate.open();
    ix.await_launch().await;

    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "the abandoned checkpoint creates no pane"
    );
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "a cancelled launch never becomes a pending spawn"
    );
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the finished preparation does not resurrect the deleted row"
    );
}
