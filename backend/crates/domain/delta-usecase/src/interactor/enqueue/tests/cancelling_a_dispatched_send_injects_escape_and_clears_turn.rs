//! Cancelling a `dispatched` send whose echo never arrived: Delta injects a
//! single `Escape` into the pane (discarding the TUI composer's buffer),
//! marks the row `cancelled`, and exits `AwaitingEcho` back to `Idle` — at
//! which point any send queued behind the cancelled head promotes naturally
//! on the next idle-flush.
//!
//! These tests pin the user-visible escape hatch for the bug that motivated
//! the path: the user pressed `Escape` in the TUI before the prompt
//! submitted, so no `UserPromptSubmit` ever fires and the row would otherwise
//! stay `dispatched` forever (the composer disabled, only a server restart
//! recovers). Cancelling from the browser reproduces the keypress for them.

use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::error::Error;
use crate::interactor::testing::*;
use crate::turn::TurnState;

/// The happy path: a dispatched send (state `AwaitingEcho`) is cancelled by
/// injecting `Escape` into the pane, the row drops to `cancelled`, and the
/// turn is back to `Idle`.
#[tokio::test]
async fn cancelling_a_dispatched_send_injects_escape_and_returns_to_idle() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A send into an idle session is dispatched immediately and the turn is
    // in `AwaitingEcho` until the `UserPromptSubmit` hook fires.
    let (send, _) = ix.enqueue_send(to(main), "go", None).await.unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id }
    );

    // The user presses Escape in the TUI (no signal arrives) and clicks
    // Cancel in the browser: Delta injects Escape on their behalf and the
    // row drops to cancelled.
    ix.cancel_send(send.id).await.unwrap();

    let keyed = ix.tmux_fake().keyed.lock().unwrap().clone();
    assert_eq!(keyed.len(), 1, "exactly one key injection, got {keyed:?}");
    let (pane, keys) = &keyed[0];
    assert_eq!(pane, "delta-seed:0.0");
    assert_eq!(keys, &["Escape"]);
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
    );
    assert_eq!(ix.live_state_for(&session).await.turn, TurnState::Idle);
    assert!(
        ix.store().open_sends(&session).await.unwrap().is_empty(),
        "the cancelled send drops out of the open-send list"
    );
}

/// FIFO head reconciliation: a send queued behind the cancelled head
/// dispatches naturally on the idle-flush the cancel triggers.
#[tokio::test]
async fn cancelling_a_dispatched_send_promotes_the_next_queued_send() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // First send is dispatched (no echo).
    let (first, _) = ix.enqueue_send(to(main), "first", None).await.unwrap();
    assert_eq!(first.status, SendStatus::Dispatched);

    // A second send mid-`AwaitingEcho` is queued behind the first.
    let parent = MessageUuid::from("uuid-parent");
    let (second, _) = ix
        .enqueue_send(branch_off(main, &parent), "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(second.status, SendStatus::Queued);
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "the queued send has not been typed yet"
    );

    // Cancel the dispatched head: the queued one promotes through the
    // idle-flush the cancel triggers.
    ix.cancel_send(first.id).await.unwrap();

    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2, "the queued send is dispatched on the cancel");
    assert_eq!(sent[1].1, "branch text");
    assert_eq!(
        ix.store().send(first.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
    );
    assert_eq!(
        ix.store().send(second.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: second.id },
    );
}

/// A `UserPromptSubmit` hook arriving after the cancel has already run (the
/// user pressed Enter a moment before the browser's cancel landed) does not
/// regress state: the cancelled row is filtered out of
/// `head_dispatched_send`, so the prompt classifies as external input and
/// the state machine moves from Idle to InFlight{None} — no stuck or
/// surprising state.
#[tokio::test]
async fn late_echo_after_cancel_is_treated_as_external_input() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix.enqueue_send(to(main), "go", None).await.unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);

    // Cancel first.
    ix.cancel_send(send.id).await.unwrap();
    assert_eq!(ix.live_state_for(&session).await.turn, TurnState::Idle);

    // Now the late `UserPromptSubmit` for the same text arrives (the user
    // had pressed Enter just before the cancel landed). The cancelled row
    // is no longer dispatched, so the hook classifies it as external input.
    ix.transcript_fake().push(user_line("u-late", "go"));
    ix.on_user_prompt_submit(submit("go")).await.unwrap();

    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight { send_id: None },
        "the late echo classifies as external input, not a stuck dispatch"
    );
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
        "the cancelled send stays cancelled (not promoted back to matched)"
    );
}

/// Cancelling once the turn has moved past `AwaitingEcho` is a conflict:
/// the echo already arrived, so the turn is owned by its transcript line
/// and the user reaches for the in-flight interrupt instead.
#[tokio::test]
async fn cancelling_an_echoed_send_in_flight_is_a_conflict() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix.enqueue_send(to(main), "go", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-go", "go"));
    ix.on_user_prompt_submit(submit("go")).await.unwrap();
    assert_ne!(ix.live_state_for(&session).await.turn, TurnState::Idle);

    let err = ix.cancel_send(send.id).await.unwrap_err();
    assert!(
        matches!(err, Error::SendNotCancellable(id) if id == send.id),
        "a cancel arriving after the echo is a conflict, got {err:?}"
    );
    assert!(
        ix.tmux_fake().keyed.lock().unwrap().is_empty(),
        "no Escape is injected when the cancel is rejected"
    );
}
