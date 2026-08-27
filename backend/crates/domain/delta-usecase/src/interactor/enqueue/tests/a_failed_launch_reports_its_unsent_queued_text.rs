use crate::interactor::testing::*;
use crate::ports::{SessionEvent, WorktreeStartPoint};
use crate::{SendTarget, WorktreeSpec};

/// A failed launch hands back every message it never delivered.
///
/// The rollback deletes the eager session row, and the `send` rows go with it
/// (`send.session_id … ON DELETE CASCADE`), so the failure event is the last
/// moment their text exists anywhere. The failed chip's Retry holds only the
/// FIRST prompt, so without this every message the user typed while the launch
/// was still checking out would be silently lost — which is exactly the window
/// the queued acceptance opened up.
///
/// So `spawn_failed` carries `unsent`: every row that never reached an agent,
/// in id order, the first prompt included. The client decides what it already
/// holds; the server re-sends nothing.
#[tokio::test]
async fn a_failed_launch_reports_its_unsent_queued_text() {
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
        unsent,
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
    assert_eq!(
        unsent
            .iter()
            .map(|send| (send.send_id, send.text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (first.id, "first message"),
            (queued.id, "and one more while it starts"),
        ],
        "every send that never reached an agent rides out, in id order"
    );

    // …and the rows really are gone, which is why the event had to carry them.
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the failed launch left no session row behind"
    );
    assert!(
        ix.store().send(first.id).await.unwrap().is_none(),
        "the first prompt's row cascaded away with the session"
    );
    assert!(
        ix.store().send(queued.id).await.unwrap().is_none(),
        "the queued row cascaded away with the session"
    );
}
