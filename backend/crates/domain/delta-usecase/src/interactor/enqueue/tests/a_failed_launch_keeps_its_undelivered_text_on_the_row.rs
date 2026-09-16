use crate::interactor::testing::*;
use crate::ports::{SessionEvent, WorktreeStartPoint};
use crate::{SendTarget, WorktreeSpec};

/// A failed launch keeps every message it never delivered — on its own row.
///
/// The ending marks the session `failed` instead of deleting it, so the `send`
/// rows it accepted (the first prompt, plus everything the user typed while the
/// checkout was still running) stay open against it and the failed session's
/// own screen reads them back from the server. Nothing has to ride out on the
/// failure event to survive, and nothing is re-sent.
#[tokio::test]
async fn a_failed_launch_keeps_its_undelivered_text_on_the_row() {
    // A worktree build that is held open (so a send can be accepted mid-launch)
    // and then fails when released (so the rollback runs).
    let gate = WorktreeGate::closed();
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree {
        fail_create: true,
        ..Default::default()
    }
    .with_repo(&canonical, repo_root)
    .with_gate(&gate);
    let (ix, mut sink) = interactor_with_git_and_event_sink(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let (first, _) = ix
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
            "first message",
            None,
        )
        .await
        .expect("the send is accepted before the launch is attempted");
    let session_id = first.session_id.clone();

    // The user keeps typing while the checkout runs; the send is accepted as a
    // `queued` row.
    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    let (queued, _) = ix
        .enqueue_send(to(main), "and one more while it starts", None)
        .await
        .expect("a plain send to a still-spawning session is accepted");

    // Release the build; it fails, and the acceptance is rolled back.
    gate.open();
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
    assert_eq!(failed_id, session_id);
    assert!(
        reason.is_some_and(|reason| reason.contains("worktree")),
        "the failed launch still names its cause"
    );
    // The row is kept, marked `failed`, with the cause recorded on it.
    let session = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the failed launch keeps its session row");
    assert_eq!(session.status, delta_model::SessionStatus::Failed);
    assert!(
        session
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("worktree")),
        "the cause is persisted on the row, so it survives a reload"
    );

    // Every send that never reached an agent is still open against it, in id
    // order — the failed session's screen reads exactly this list.
    assert_eq!(
        ix.store()
            .open_sends(&session_id)
            .await
            .unwrap()
            .into_iter()
            .map(|send| (send.id, send.text))
            .collect::<Vec<_>>(),
        vec![
            (first.id, "first message".to_owned()),
            (queued.id, "and one more while it starts".to_owned()),
        ],
    );
}
