//! The incident regression: a dispatched send whose keystrokes vanish without
//! producing a single signal must not wedge the session forever.
//!
//! Claude Code put up its own interactive dialog between turns. Delta typed a
//! send into the pane; the dialog swallowed the pasted text whole (it reached
//! no scrollback, composer, or transcript) and answered itself with the
//! trailing Enter. No user message was written, no `UserPromptSubmit` fired, no
//! turn started or ended — so every event-driven recovery Delta has (mismatched
//! echo, turn end, compact summary) had nothing to react to. The row sat
//! `dispatched`, the browser showed a permanent "In progress", and the next
//! send stayed `queued` behind it until a human intervened.
//!
//! The echo deadline is the net underneath that: after a bounded wait the
//! silence itself becomes a turn input. These tests pin both halves of it —
//! the retry that self-heals, and the park that hands the text back and frees
//! the queue.

use std::time::{Duration, Instant};

use delta_model::{SendStatus, SessionId};

use crate::interactor::session_actor::runtime::ECHO_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

/// The pane the seeded session's keystrokes go into.
const PANE: &str = "delta-seed:0.0";

/// An instant far enough past `from` that any pending echo deadline has
/// expired. The stamp is taken from the live clock when the wait begins, so a
/// test never sleeps: it simply sweeps with a `now` from the future.
fn past_deadline(from: Instant) -> Instant {
    from + ECHO_DEADLINE + Duration::from_secs(1)
}

/// A send swallowed twice: the first deadline re-types it behind an `Escape`,
/// the second parks it with its text handed back — and the send queued behind
/// it dispatches instead of waiting forever.
#[tokio::test]
async fn swallowed_send_is_retyped_then_parked_by_the_echo_deadline() {
    let (ix, mut events) = interactor_with_event_sink();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The message the dialog eats. Nothing will ever be heard about it: no
    // hook, no transcript line, no turn boundary.
    let (send, _) = ix
        .enqueue_send(to(main), "swallowed by a dialog", None)
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id },
    );

    // A second message composed while the first is outstanding waits its turn
    // — the queue this incident used to strand.
    let (behind, _) = ix
        .enqueue_send(to(main), "waiting behind", None)
        .await
        .unwrap();
    assert_eq!(behind.status, SendStatus::Queued);

    // A sweep before the deadline changes nothing: the wait is still young.
    let dispatched = ix.sweep_echo_deadlines(Instant::now()).await.unwrap();
    assert!(dispatched.is_empty(), "no deadline has passed yet");
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );

    // FIRST DEADLINE: the turn is released and the send goes back to `queued`,
    // then re-types in the same tick — preceded by a single `Escape`, so a
    // dialog still up is dismissed and a half-landed composer draft discarded
    // before the text lands again.
    let dispatched = ix
        .sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    assert!(
        dispatched.contains(&SessionEvent::SendDispatched {
            session_id: session.clone(),
            send_id: send.id,
        }),
        "the re-dispatch is announced so the browser keeps the chip honest; \
         got {dispatched:?}"
    );
    assert_eq!(
        ix.tmux_fake().pane_input.lock().unwrap().clone(),
        vec![
            PaneInput::Line {
                pane: PANE.to_owned(),
                text: "swallowed by a dialog".to_owned(),
            },
            PaneInput::Keys {
                pane: PANE.to_owned(),
                keys: vec!["Escape".to_owned()],
            },
            PaneInput::Line {
                pane: PANE.to_owned(),
                text: "swallowed by a dialog".to_owned(),
            },
        ],
        "the retry is an Escape followed by the same text, into the same pane",
    );
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the retry is outstanding again",
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id },
    );
    assert_eq!(
        ix.store().send(behind.id).await.unwrap().unwrap().status,
        SendStatus::Queued,
        "the send behind it keeps its place; the retry is still the head",
    );

    // SECOND DEADLINE: the retry was swallowed too, so the budget is spent.
    // The send is parked — cancelled, out of the open list, its text handed
    // back — and the queue behind it moves.
    let dispatched = ix
        .sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    assert!(
        dispatched.contains(&SessionEvent::SendDispatched {
            session_id: session.clone(),
            send_id: behind.id,
        }),
        "the send queued behind the parked one is dispatched; got {dispatched:?}"
    );

    let parked = ix.store().send(send.id).await.unwrap().unwrap();
    assert_eq!(parked.status, SendStatus::Cancelled, "parked, not retried");
    let open = ix.store().open_sends(&session).await.unwrap();
    assert!(
        !open.iter().any(|s| s.id == send.id),
        "the parked send leaves the open list, so its pending chip clears",
    );
    let mut drained = Vec::new();
    while let Ok(event) = events.try_recv() {
        drained.push(event);
    }
    assert!(
        drained.contains(&SessionEvent::SendParked {
            session_id: session.clone(),
            send_id: send.id,
            text: "swallowed by a dialog".to_owned(),
        }),
        "the park hands the composed text back rather than dropping it; got {drained:?}"
    );

    // Exactly one retry, and the queue is moving again: the text was typed
    // twice (never a third time) and the next send is now outstanding.
    let typed = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(
        typed
            .iter()
            .filter(|(_, text)| text == "swallowed by a dialog")
            .count(),
        2,
        "dispatched once, re-dispatched exactly once",
    );
    assert_eq!(typed.last().unwrap().1, "waiting behind");
    assert_eq!(
        ix.store().send(behind.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: behind.id },
    );
    assert_eq!(
        ix.tmux_fake().keyed.lock().unwrap().len(),
        1,
        "only the retry injects an Escape; the park has nothing to clear for",
    );
}

/// The other half — and the outcome production should see most often: the
/// dialog is gone by the time the retry lands, so the re-typed send echoes and
/// matches, and the user only ever sees a late answer.
///
/// Worth pinning separately because the retry is not the same keystroke path as
/// the first dispatch: it goes back through `queued`, is re-promoted, and is
/// typed behind an `Escape`. If any of that left the row uncorrelatable, the
/// send would still be lost — just one deadline later, and by then the budget
/// is spent, so the park would be the ONLY outcome the watchdog could ever
/// produce.
#[tokio::test]
async fn a_retyped_send_still_matches_its_echo_and_self_heals() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix.enqueue_send(to(main), "answer me", None).await.unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);

    // The first keystrokes are swallowed, so the deadline re-types the send.
    ix.sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id },
        "the retry is outstanding",
    );

    // This time nothing eats it: the prompt submits and its echo comes back.
    ix.transcript_fake().push(user_line("u-1", "answer me"));
    let (events, _) = ix.on_user_prompt_submit(submit("answer me")).await.unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnStarted { send_id, .. } if *send_id == send.id
        )),
        "the re-typed send is credited with the turn it started; got {events:?}"
    );
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
        "a re-typed send correlates exactly like a first dispatch",
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(send.id)
        },
    );

    // Typed twice, never a third time, and no deadline is left to fire: the
    // send healed instead of walking on to the park.
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        2,
        "the original dispatch and the one retry",
    );
    let dispatched = ix
        .sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    assert!(dispatched.is_empty(), "got {dispatched:?}");
    assert_ne!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
        "a healed send is never parked afterwards",
    );
}

/// A deadline that arrives after the echo already matched is a stale no-op: a
/// normal send is never re-typed by a sweep tick that raced its own settle.
#[tokio::test]
async fn echo_deadline_after_a_matched_echo_never_retypes() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix.enqueue_send(to(main), "answer me", None).await.unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);

    // The echo lands normally, well inside the deadline: the turn is in flight
    // and owned by its transcript line.
    ix.transcript_fake().push(user_line("u-1", "answer me"));
    ix.on_user_prompt_submit(submit("answer me")).await.unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(send.id)
        },
    );

    // A sweep tick from far in the future — one that would have fired had the
    // wait still been open — finds nothing to do.
    let dispatched = ix
        .sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    assert!(dispatched.is_empty(), "got {dispatched:?}");
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "the send is typed exactly once: a stale deadline never double-types",
    );
    assert!(
        ix.tmux_fake().keyed.lock().unwrap().is_empty(),
        "and no Escape is injected into a healthy in-flight turn",
    );
    assert_ne!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
        "a matched send is never parked by a racing deadline",
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(send.id)
        },
        "and the running turn is left alone",
    );
}

/// A first prompt held for a resuming session is not measured by this deadline:
/// its keystrokes are deliberately *not* in the pane yet (typing into a pane
/// that is not accepting input would lose them), so there is nothing that could
/// have gone missing. The wait starts when the resume settles and the prompt is
/// actually typed.
#[tokio::test]
async fn a_held_resume_prompt_is_not_swept_by_the_echo_deadline() {
    let ix = interactor();
    // A known-but-closed session: the send resumes it and holds the keystroke.
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let session = SessionId::from("sess-R");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix
        .enqueue_send(to(main), "after resume", None)
        .await
        .unwrap();
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "the first prompt is held until the resume is ready",
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id },
    );

    // A sweep long past the deadline leaves the held prompt exactly as it is:
    // no requeue, no park, no Escape into a pane that is still coming up.
    let dispatched = ix
        .sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    assert!(dispatched.is_empty(), "got {dispatched:?}");
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );
    assert!(ix.tmux_fake().keyed.lock().unwrap().is_empty());
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id },
    );
}
