use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::StopHook;

/// A turn started by input typed straight into the embedded pane — not
/// dispatched by Delta — still marks the session busy, because its
/// `UserPromptSubmit` hook reaches Delta. A composer branch/quoted send arriving
/// during it must defer just as it would during a Delta-dispatched turn, so its
/// locator quote is not lost to Claude Code's mid-turn queueing. When the
/// external turn ends, the queued send is dispatched.
#[tokio::test]
async fn branch_send_during_external_turn_is_queued() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A prompt typed straight into the pane: external input (no Delta dispatch),
    // with no Stop yet — the turn is in flight.
    ix.on_user_prompt_submit(submit("typed in the pane"))
        .await
        .unwrap();
    assert_ne!(
        ix.turn_state_for(&session).await,
        crate::turn::TurnState::Idle,
        "a pane-typed turn marks the session busy via its hook"
    );
    let sent_before = ix.tmux_fake().sent.lock().unwrap().len();

    // A composer branch send during that external turn must defer.
    let parent = MessageUuid::from("uuid-parent");
    let (queued, _) = ix
        .enqueue_send(branch_off(main, &parent), "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(queued.status, SendStatus::Queued);
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        sent_before,
        "no keystrokes are dispatched while queued"
    );

    // The external turn ends: the queued send is dispatched.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    let dispatched = {
        let sent = ix.tmux_fake().sent.lock().unwrap();
        sent.last().map(|p| p.1.clone())
    };
    assert_eq!(dispatched.as_deref(), Some("branch text"));
    assert!(ix
        .store()
        .next_queued_send(&session)
        .await
        .unwrap()
        .is_none());
}
