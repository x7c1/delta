//! Cancelling a still-`queued` send transitions it to `cancelled` and the idle
//! dispatch path then skips it, so it never reaches the pane.
//!
//! A send composed while a turn is in flight is held `queued`; the user can
//! abandon it before it dispatches. These tests assert the happy path (a queued
//! send cancels and is not dispatched when the turn ends) and the guards (an
//! unknown send is a clean `SendNotCancellable` conflict). The
//! `dispatched`-cancel path (Escape injection while `AwaitingEcho`) is covered
//! in `cancelling_a_dispatched_send_injects_escape_and_clears_turn`.

use delta_model::{SendStatus, SessionId};

use crate::error::Error;
use crate::interactor::testing::*;
use crate::ports::StopHook;

#[tokio::test]
async fn cancelling_a_queued_send_drops_it_from_the_idle_dispatch() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Start a turn: dispatch and echo the first send.
    ix.enqueue_send(to(main), "first", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-first", "first"));
    ix.on_user_prompt_submit(submit("first")).await.unwrap();
    assert_ne!(
        ix.live_state_for(&session).await.turn,
        crate::turn::TurnState::Idle
    );

    // A second send mid-turn is held queued, not typed into the pane.
    let (second, _) = ix.enqueue_send(to(main), "second", None).await.unwrap();
    assert_eq!(second.status, SendStatus::Queued);

    // The user cancels the queued send before it dispatches.
    ix.cancel_send(second.id).await.unwrap();
    assert!(
        ix.store()
            .open_sends(&session)
            .await
            .unwrap()
            .iter()
            .all(|s| s.id != second.id),
        "a cancelled send drops out of the open-send list"
    );

    // The turn ends: the idle dispatch path skips the cancelled send.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "the cancelled send was never dispatched (only the first turn's keystrokes)"
    );
    assert!(
        ix.store()
            .next_queued_send(&session)
            .await
            .unwrap()
            .is_none(),
        "no queued send remains to dispatch"
    );
}

#[tokio::test]
async fn cancelling_an_unknown_send_is_a_conflict() {
    let ix = interactor();
    ix.seed_session().await;

    let err = ix.cancel_send(9999).await.unwrap_err();
    assert!(
        matches!(err, Error::SendNotCancellable(9999)),
        "an unknown send id is a conflict, got {err:?}"
    );
}
