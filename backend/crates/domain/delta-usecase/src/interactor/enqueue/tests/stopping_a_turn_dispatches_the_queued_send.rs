//! Stopping a Claude session's in-flight turn from the browser.
//!
//! Claude Code can end a turn without any signal Delta sees (Escape pressed
//! after the prompt submitted but before any reply streamed: no `Stop` hook,
//! no transcript interrupt marker), leaving the turn machine `InFlight` and
//! every later send `queued` forever. `stop_turn` presses Escape and then
//! Ctrl+C in the pane (emptying the composer Claude Code restored the prompt
//! into), ends the turn the way an ingested interrupt marker does, announces
//! `TurnInterrupted`, and dispatches the next queued send.
//!
//! The success paths are pinned here — both `InFlight` variants, a failed
//! key injection, and the late interrupt marker Claude Code writes when the
//! stop interrupted a streaming reply. The refusals live in
//! `stopping_a_turn_is_refused_without_an_in_flight_turn`.

use delta_model::{SendStatus, SessionId, ThreadId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

/// Assert the stop succeeded with exactly the expected broadcast: the
/// interrupted turn on `thread`, then the queued send going out.
fn assert_stopped(events: &[SessionEvent], session: &SessionId, thread: ThreadId, next: i64) {
    assert!(
        matches!(
            events,
            [
                SessionEvent::TurnInterrupted { session_id, thread_id: Some(t) },
                SessionEvent::SendDispatched { session_id: dispatched_in, send_id },
            ] if session_id == session && *t == thread && dispatched_in == session && *send_id == next
        ),
        "expected TurnInterrupted then SendDispatched, got {events:?}"
    );
}

/// Assert exactly one injection, `Escape` then `C-c`, went to the seeded
/// session's pane.
fn assert_stop_keys_injected(ix: &TestInteractor) {
    let keyed = ix.tmux_fake().keyed.lock().unwrap().clone();
    assert_eq!(keyed.len(), 1, "exactly one key injection, got {keyed:?}");
    let (pane, keys) = &keyed[0];
    assert_eq!(pane, "delta-seed:0.0");
    assert_eq!(keys, &["Escape", "C-c"]);
}

/// Seed a session whose turn carries a Delta send that echoed, with a second
/// send queued behind it. Returns the session, its main thread, and the two
/// send ids.
async fn in_flight_send_with_a_queued_one(ix: &TestInteractor) -> (SessionId, ThreadId, i64, i64) {
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();
    let (first, _) = ix.enqueue_send(to(main), "first", None).await.unwrap();
    // The echo arrives, but no transcript line ever follows it.
    ix.on_user_prompt_submit(submit("first")).await.unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(first.id)
        }
    );
    let (second, _) = ix.enqueue_send(to(main), "second", None).await.unwrap();
    assert_eq!(second.status, SendStatus::Queued);
    (session, main, first.id, second.id)
}

/// The reproduced case: a Delta send echoed (the turn is `InFlight` carrying
/// it), then Claude dropped the prompt silently. The stop presses Escape and
/// Ctrl+C, ends the turn, settles the send as delivered, and types the send
/// queued behind it.
#[tokio::test]
async fn stopping_a_turn_carrying_a_send_dispatches_the_queued_send() {
    let ix = interactor();
    let (session, main, first, second) = in_flight_send_with_a_queued_one(&ix).await;

    let events = ix.stop_turn(&session).await.unwrap();

    assert_stopped(&events, &session, main, second);
    assert_stop_keys_injected(&ix);
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2, "the queued send is typed on the stop");
    assert_eq!(sent[1].1, "second");
    assert_eq!(
        ix.store().send(first).await.unwrap().unwrap().status,
        SendStatus::Matched,
        "the stopped turn's send settles as delivered, as on an interrupt"
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho {
            send_id: second,
            slash_command: false,
        }
    );
}

/// A turn started by input typed in the pane (`InFlight` with no send) is
/// stopped the same way.
#[tokio::test]
async fn stopping_an_external_turn_dispatches_the_queued_send() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    ix.transcript_fake()
        .push(user_line("u-ext", "typed in the pane"));
    ix.on_user_prompt_submit(submit("typed in the pane"))
        .await
        .unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight { send_id: None }
    );
    let (queued, _) = ix.enqueue_send(to(main), "next", None).await.unwrap();
    assert_eq!(queued.status, SendStatus::Queued);

    let events = ix.stop_turn(&session).await.unwrap();

    assert_stopped(&events, &session, main, queued.id);
    assert_stop_keys_injected(&ix);
    assert_eq!(
        ix.store().send(queued.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );
}

/// When the keys cannot reach the pane nothing reached Claude either, so
/// the turn is not reported as stopped: it stays `InFlight`, no event goes
/// out, and the queued send stays queued.
#[tokio::test]
async fn a_failed_escape_injection_leaves_the_turn_in_flight() {
    let ix = interactor();
    let (session, _, first, second) = in_flight_send_with_a_queued_one(&ix).await;
    ix.tmux_fake().fail_key_injection();

    ix.stop_turn(&session)
        .await
        .expect_err("the failed injection surfaces");

    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(first)
        }
    );
    assert_eq!(ix.tmux_fake().sent.lock().unwrap().len(), 1);
    assert_eq!(
        ix.store().send(second).await.unwrap().unwrap().status,
        SendStatus::Queued,
    );
}

/// Stopping a turn Claude was streaming makes Claude Code write an interrupt
/// marker a moment later. By then the queued send is already awaiting its
/// echo; the marker belongs to the stopped turn, so it neither announces a
/// second `TurnInterrupted` nor requeues (and re-types) that send. A marker
/// after the next prompt belongs to the new turn and ends it as usual.
#[tokio::test]
async fn the_late_interrupt_marker_of_a_stopped_turn_is_not_a_second_interrupt() {
    let (ix, mut async_events) = interactor_with_event_sink();
    let (session, _, _, second) = in_flight_send_with_a_queued_one(&ix).await;
    ix.stop_turn(&session).await.unwrap();
    while async_events.try_recv().is_ok() {}

    ix.transcript_fake()
        .push(interrupt_line("uuid-late-marker"));
    ix.poll_transcript(TICK_BOUND).await;

    let mut announced = Vec::new();
    while let Ok(event) = async_events.try_recv() {
        announced.push(event);
    }
    assert!(
        !announced.iter().any(|event| matches!(
            event,
            SessionEvent::TurnInterrupted { .. } | SessionEvent::SendDispatched { .. }
        )),
        "the stopped turn's marker is not announced again, got {announced:?}"
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho {
            send_id: second,
            slash_command: false,
        },
        "the queued send stays outstanding"
    );
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        2,
        "the queued send is typed once"
    );

    // The queued send's turn starts; a marker now interrupts THAT turn.
    ix.on_user_prompt_submit(submit("second")).await.unwrap();
    ix.transcript_fake()
        .push(interrupt_line("uuid-next-marker"));
    ix.poll_transcript(TICK_BOUND).await;

    let mut announced = Vec::new();
    while let Ok(event) = async_events.try_recv() {
        announced.push(event);
    }
    assert!(
        announced
            .iter()
            .any(|event| matches!(event, SessionEvent::TurnInterrupted { .. })),
        "a marker of the next turn ends it, got {announced:?}"
    );
    assert_eq!(ix.live_state_for(&session).await.turn, TurnState::Idle);
}
