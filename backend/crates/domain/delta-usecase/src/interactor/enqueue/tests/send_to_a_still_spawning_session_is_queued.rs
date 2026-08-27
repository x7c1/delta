use delta_model::{MessageUuid, SendStatus};

use crate::error::Error;
use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A session is listed — and so reachable from the browser — from the moment
/// its first send is accepted, while its launch is still coming up. A **plain**
/// send arriving in that window is *accepted* as a `queued` row rather than
/// refused: the launch is where the wait lives (a PR-origin session spends the
/// whole `git fetch` + checkout there), and a user who already knows their next
/// message should not have to hold it until the checkout finishes.
///
/// What must NOT happen is a dispatch: no pane is bound to the session yet and
/// its transcript does not exist, so the ordinary closed-session path
/// (`ensure_open` → `claude --resume <id>`) would launch a SECOND agent against
/// a conversation the first launch has not written. The queued row is the whole
/// point — it records the message without touching the launch.
///
/// A **branch** send is the one shape still refused: it names a message to
/// branch from, and a session that has never bound has ingested none.
#[tokio::test]
async fn send_to_a_still_spawning_session_is_queued() {
    // The worktree gate holds the launch preparation open, so the assertions
    // below stand squarely inside the accept→launch window instead of racing
    // the background task.
    let gate = WorktreeGate::closed();
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
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
        .unwrap();
    let session_id = first.session_id.clone();
    assert_eq!(
        ix.launching_session_ids().await,
        vec![session_id.clone()],
        "the launch is still in flight"
    );

    let main = ix.store().main_thread_id(&session_id).await.unwrap();

    // The plain send is accepted, as a `queued` row on the session's main
    // thread — the same state a send composed mid-turn takes.
    let (queued, events) = ix
        .enqueue_send(to(main), "and one more while it starts", None)
        .await
        .expect("a plain send to a still-starting session is accepted");
    assert_eq!(queued.session_id, session_id);
    assert_eq!(queued.thread_id, main);
    assert_eq!(queued.status, SendStatus::Queued);
    assert_eq!(queued.text, "and one more while it starts");

    // No event announces it. There is no `send_queued` kind: the 201 body above
    // and `GET /api/sessions/{id}/sends` are how a queued row reaches clients.
    assert!(
        events.is_empty(),
        "accepting a queued send announces nothing, got {events:?}"
    );
    assert!(
        sink.try_recv().is_err(),
        "nothing is broadcast on the async seam either"
    );

    // And nothing was launched or typed: the accepted row did not touch the
    // launch, so no rival agent was started and no keystroke went out.
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no second agent was launched"
    );
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "no keystrokes were dispatched"
    );

    // A branch send in the same state has nothing to branch from, so it is
    // still refused with the code the browser words as "still starting".
    let err = ix
        .enqueue_send(
            branch_off(main, &MessageUuid::from("no-such-message")),
            "branch from nothing",
            Some("a quote"),
        )
        .await
        .expect_err("a branch send to a still-starting session must fail");
    assert!(
        matches!(err, Error::SessionSpawning(ref id) if id == session_id.as_str()),
        "the refusal propagates as SessionSpawning, got: {err:?}"
    );

    // The refused branch send left no row: the two accepted sends are the whole
    // open list, oldest first.
    let open = ix.store().open_sends(&session_id).await.unwrap();
    assert_eq!(
        open.iter()
            .map(|send| (send.text.as_str(), send.status))
            .collect::<Vec<_>>(),
        vec![
            ("first message", SendStatus::Dispatched),
            ("and one more while it starts", SendStatus::Queued),
        ],
        "the first prompt and the queued follow-up, and nothing from the branch"
    );

    // The held launch still completes normally once released.
    gate.open();
    ix.await_launch().await;
    assert_eq!(ix.pending_session_ids().await, vec![session_id]);
}
