use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::SendTarget;

/// When the composer-first spawn cannot launch its tmux session
/// (`create_session` fails), the send is still *accepted* — the launch runs in
/// the background, after the response has gone out — and the failure is
/// reported afterwards: the eager row is rolled back, a `spawn_failed` naming
/// the tmux error is broadcast, and nothing is left pending, so a later,
/// unrelated `UserPromptSubmit` is not mis-bound to the abandoned pane (it
/// registers as external input instead).
#[tokio::test]
async fn composer_first_send_rolls_back_a_failed_launch() {
    let (ix, mut sink) = interactor_with_failing_create_session_and_event_sink();

    // No session yet: the composer-first send is accepted with real ids. The
    // launch it hands to the background task is the thing that fails.
    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "first message",
            None,
        )
        .await
        .expect("the send is accepted before the launch is attempted");
    ix.await_launch().await;

    // The failure arrives on the async seam instead of as an error response,
    // carrying the tmux message — the only place it can still reach the user.
    let event = sink.try_recv().expect("a spawn failure was broadcast");
    let SessionEvent::SpawnFailed {
        session_id, reason, ..
    } = event
    else {
        panic!("expected SpawnFailed, got {event:?}");
    };
    assert_eq!(session_id, send.session_id);
    assert!(
        reason.is_some_and(|reason| reason.contains("create failed")),
        "the failed launch names its cause"
    );

    // The launch was rolled back: nothing is pending, so a later hook carrying
    // any session id finds none to bind and is treated as external input on a
    // closed session rather than binding to the abandoned pane.
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "the failed launch left no pending entry behind"
    );
    // The eager session row (and its send, by cascade) was rolled back too.
    // Checked against the fake store's raw rows so the still-`spawning` eager
    // row would be caught too (the session-list page deliberately hides
    // message-less spawning sessions, so it could not distinguish a lingering
    // one from a deleted one).
    assert!(
        ix.store().inner.lock().unwrap().sessions.is_empty(),
        "the failed launch left no session row behind"
    );
    let (events, _) = ix
        .on_user_prompt_submit(submit_in(
            "sess-late",
            "/work/delta-1/t.jsonl",
            "/work/delta-1",
            "typed in claude",
        ))
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "no pending spawn remained, so the hook is external input"
    );
    assert!(
        ix.pane_for_session(&SessionId::from("sess-late"))
            .await
            .is_none(),
        "the rolled-back launch must not bind a later session"
    );
}
