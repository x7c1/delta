use crate::interactor::testing::*;
use crate::ports::{AsyncEventSink, SessionEvent};
use crate::SendTarget;

/// A `create_session` that fails still rolls the acceptance back, even though
/// the spawn was recorded before it ran.
///
/// The pending spawn is now recorded *before* the pane is created — that is what
/// lets the launch's first hook bind — so a tmux failure is the one case where
/// the rollback has to remove a record the launch itself installed. Missing it
/// would leave a pending spawn pointing at a pane that never came up: the
/// session would sit `spawning` until the bind watchdog reaped it, and any
/// later hook could be mis-bound to the abandoned token in the meantime.
#[tokio::test]
async fn tmux_failure_after_the_pending_spawn_is_recorded_reaps_the_row() {
    let (sink, mut events) = AsyncEventSink::channel();
    let gate = TmuxGate::closed();
    let ix = interactor_with_tmux(FakeTmux {
        fail_create: true,
        ..FakeTmux::default().with_gate(&gate)
    })
    .with_event_sink(sink);

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
    let session_id = send.session_id.clone();

    // The launch reached `create_session`, so the spawn is on the books…
    gate.await_entered().await;
    assert!(
        ix.with_runtime(&session_id, |state| state.pending_spawn().is_some())
            .await,
        "the pending spawn is recorded before the pane is attempted"
    );

    // …and the attempt fails.
    gate.open();
    ix.await_launch().await;

    let event = events.try_recv().expect("a spawn failure was broadcast");
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
        reason.is_some_and(|reason| reason.contains("create failed")),
        "the failure carries tmux's message"
    );

    // Nothing is left behind: not the eager row, and not the spawn recorded a
    // moment before the failure.
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the eager row of a failed launch is deleted"
    );
    let (launching, pending) = ix
        .with_runtime(&session_id, |state| {
            (
                state.launching_spawn().is_some(),
                state.pending_spawn().is_some(),
            )
        })
        .await;
    assert!(!launching, "no launching entry survives the failure");
    assert!(
        !pending,
        "the spawn recorded for the pane that never came up is removed"
    );
}
