use delta_model::SessionStatus;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::SendTarget;

/// A launch's first hook binds even when it arrives before `create_session`
/// has returned.
///
/// `tmux new-session` returns once the pane's command has been *started*, so
/// the launched agent is already running while the launch task is still inside
/// the call — and an agent that auto-submits its launch prompt (or a test
/// binary, which is instant) can fire `SessionStart`/`UserPromptSubmit` from
/// there. Those hooks bind the `PendingSpawn`, so the spawn has to be recorded
/// before the pane exists; recording it after `create_session` returned leaves
/// the hook nothing to bind, it is written off as external input, and the
/// session never activates — no pane, no transcript path, no ingestion.
///
/// With `create_session` held open this test stands in exactly that window:
/// the spawn is already pending, the hook binds it, and the launch's own
/// completion afterwards changes nothing.
#[tokio::test]
async fn session_start_during_the_pane_launch_binds_the_spawn() {
    let gate = TmuxGate::closed();
    let ix = interactor_with_tmux(FakeTmux::default().with_gate(&gate));

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .expect("the send is accepted before the launch runs");
    let session_id = send.session_id.clone();

    // Stand in the window the fix is about: the launch is inside
    // `create_session`, so the pane's command is as good as running…
    gate.await_entered().await;
    let (launching, pending) = ix
        .with_runtime(&session_id, |state| {
            (
                state.launching_spawn().is_some(),
                state.pending_spawn().is_some(),
            )
        })
        .await;
    // …and the spawn the hook needs is already recorded, not still launching.
    assert!(
        !launching,
        "the launching entry is settled before the pane is created"
    );
    assert!(
        pending,
        "the pending spawn is recorded before the pane is created"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "the pane has not been created yet"
    );

    // The hook the launch triggers finds its spawn and binds it.
    let events = ix
        .on_session_start(session_start(session_id.as_str(), "startup"))
        .await
        .unwrap();
    assert!(
        events.contains(&SessionEvent::SessionRegistered {
            session_id: session_id.clone(),
        }),
        "the hook registered the session instead of being written off"
    );
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        Some("delta-1:0.0".to_owned()),
        "the hook bound the spawn's pane"
    );

    // Letting the launch finish is a no-op: its `LaunchFinished` finds the
    // spawn already bound and leaves the session alone.
    gate.open();
    ix.await_launch().await;

    assert_eq!(ix.tmux_fake().created.lock().unwrap().len(), 1);
    assert!(
        ix.is_session_open(&session_id).await,
        "the bound session stays open once the launch reports back"
    );
    let session = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the session row survives the launch");
    assert_eq!(
        session.status,
        SessionStatus::Active,
        "the session activated on its first hook"
    );
}
