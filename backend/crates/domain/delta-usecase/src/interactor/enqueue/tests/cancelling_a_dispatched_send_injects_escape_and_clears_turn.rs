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

/// Cancelling a `dispatched` row the turn machine holds no claim on (here:
/// the turn is `Idle` — the ownerless-zombie state a dead process could
/// leave, normally cleared by the boot reconcile) is a pure state
/// transition: the row flips to `cancelled`, no keystroke is injected (there
/// is no composer buffer Delta knows about), and the turn state is
/// untouched.
#[tokio::test]
async fn cancelling_an_ownerless_dispatched_send_is_a_pure_state_transition() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Seed the zombie straight into the store: a `dispatched` row with no
    // turn machine awaiting its echo (the turn stays Idle).
    let zombie = ix
        .store()
        .enqueue_send(&session, main, None, "zombie", None)
        .await
        .unwrap();
    assert_eq!(zombie.status, SendStatus::Dispatched);
    assert_eq!(ix.live_state_for(&session).await.turn, TurnState::Idle);

    ix.cancel_send(zombie.id).await.unwrap();

    assert_eq!(
        ix.store().send(zombie.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
    );
    assert!(
        ix.tmux_fake().keyed.lock().unwrap().is_empty(),
        "no keystrokes are injected for an ownerless cancel"
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::Idle,
        "the turn machine is untouched"
    );
}

/// Same escape hatch while the turn machine is busy with a DIFFERENT send:
/// the ownerless row cancels as a pure state transition and the outstanding
/// dispatch (and its pending keystroke count) is unaffected.
#[tokio::test]
async fn ownerless_cancel_leaves_an_unrelated_outstanding_dispatch_alone() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A real outstanding dispatch owns the turn...
    let (owned, _) = ix.enqueue_send(to(main), "owned", None).await.unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: owned.id }
    );
    // ...and a zombie `dispatched` row exists beside it (an invariant
    // violation, seeded store-side).
    let zombie = ix
        .store()
        .enqueue_send(&session, main, None, "zombie", None)
        .await
        .unwrap();

    ix.cancel_send(zombie.id).await.unwrap();

    assert_eq!(
        ix.store().send(zombie.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
    );
    assert_eq!(
        ix.store().send(owned.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the owned outstanding send is untouched"
    );
    assert!(
        ix.tmux_fake().keyed.lock().unwrap().is_empty(),
        "no Escape reaches the pane"
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: owned.id },
        "the turn machine still awaits the owned send's echo"
    );
}

/// The `InFlight` rejection is keyed on the turn *carrying this send's id*,
/// not on the row's status alone: an echoed send whose user line has not
/// been ingested yet is still `dispatched` in the store, but its turn is in
/// flight and owns it — the cancel is a conflict, not an ownerless sweep.
#[tokio::test]
async fn cancelling_a_still_dispatched_send_owned_by_an_in_flight_turn_is_a_conflict() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix.enqueue_send(to(main), "go", None).await.unwrap();
    // The echo arrives but its transcript line has NOT been written yet (the
    // common timing case), so the row stays `dispatched` while the turn
    // moves to InFlight{Some(send)}.
    ix.on_user_prompt_submit(submit("go")).await.unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(send.id)
        }
    );
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );

    let err = ix.cancel_send(send.id).await.unwrap_err();
    assert!(
        matches!(err, Error::SendNotCancellable(id) if id == send.id),
        "an in-flight-owned dispatched row is a conflict, got {err:?}"
    );
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the row is not swept by the ownerless path"
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
