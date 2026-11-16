use crate::interactor::testing::*;
use crate::ports::{AsyncEventSink, SessionEvent, WorktreeStartPoint};
use crate::{SendTarget, WorktreeSpec};

/// A launch whose directory is gone by the time the pane would be created fails
/// instead of creating the pane: tmux would otherwise start it in the home
/// directory, running the agent somewhere the user never chose.
///
/// Here the branch is reported checked out in a worktree whose directory no
/// longer exists, so the launch plans to reuse that path. The launch must not
/// reach `create_session`; the session ends `failed`, and the `spawn_failed`
/// reason names the missing directory.
#[tokio::test]
async fn a_launch_into_a_missing_directory_fails_without_a_pane() {
    let canonical = FakeWorkspace::canonical("/projects/app");
    let vanished = "/scratch/gone-worktree";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, "/projects/app")
        .with_branch_checked_out("feature", vanished);
    let workspace = FakeWorkspace::default().with_missing_dir(vanished);
    workspace
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());
    let (sink, mut events) = AsyncEventSink::channel();
    let ix = interactor_with_workspace_and_git(workspace, git).with_event_sink(sink);

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: "/projects/app".to_owned(),
                launch_option_ids: Vec::new(),
                worktree: Some(WorktreeSpec::new(WorktreeStartPoint::UseRemoteBranch(
                    "feature".to_owned(),
                ))),
            },
            "hello",
            None,
        )
        .await
        .expect("the send is accepted before the launch is attempted");
    let session_id = send.session_id.clone();
    ix.await_launch().await;

    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no pane is created in a directory that does not exist"
    );

    let event = events.try_recv().expect("a spawn failure was broadcast");
    let SessionEvent::SpawnFailed {
        session_id: failed_id,
        reason,
        ..
    } = event
    else {
        panic!("expected SpawnFailed, got {event:?}");
    };
    assert_eq!(failed_id, session_id);
    assert!(
        reason.as_deref().is_some_and(|r| r.contains(vanished)),
        "the failure names the missing directory, got {reason:?}"
    );

    let stored = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the row of a failed launch is kept");
    assert_eq!(stored.status, delta_model::SessionStatus::Failed);
}
