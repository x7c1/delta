use crate::interactor::testing::*;
use crate::ports::{SessionEvent, WorktreeStartPoint};
use crate::{SendTarget, WorktreeSpec};

/// A launch preparation that fails after the send was accepted reports itself.
///
/// The send is answered before the worktree is built, so a failing
/// `git worktree add` can no longer be a `5xx` body: by the time it happens the
/// browser already holds a real session id and is showing the session starting.
/// The failure therefore arrives on the async event seam as a `spawn_failed`
/// carrying git's message as its `reason` — the only place that text can still
/// reach the user — and the eager row (with its first send, by cascade) is
/// deleted, leaving nothing launched behind it.
#[tokio::test]
async fn failed_launch_preparation_reaps_the_row_and_reports_spawn_failed() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree {
        // `git worktree add` fails, as it does for a start point that does not
        // exist on the remote.
        fail_create: true,
        ..FakeGitWorktree::default()
            .with_repo(&canonical, repo_root)
            .with_origin_url(repo_root, "https://github.com/x7c1/delta.git")
    };
    let (ix, mut sink) = interactor_with_git_and_event_sink(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    // The send is accepted with real ids: the launch has not been attempted yet.
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
        .expect("the send is accepted before the launch is attempted");
    let session_id = send.session_id.clone();
    assert_ne!(send.id, 0, "the accepted send carries a real row id");

    ix.await_launch().await;

    // The failure is announced on the seam, naming its cause.
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
    assert!(
        reason.is_some_and(|reason| reason.contains("worktree add failed")),
        "the failure carries git's message, which no response body can carry now"
    );

    // The contentless row is gone, so the session stops being listed…
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the eager row of a failed launch is deleted"
    );
    // …and nothing was launched: the build failed before the agent started.
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a failed preparation launches no agent"
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
