//! The other half of the park: a send the echo deadline gave up on stays in
//! the queue, and the user's explicit release delivers it — once.
//!
//! Parking used to cancel the row, leaving the composed text alive only inside
//! the `send_parked` broadcast: a reload, a second tab, or a session switch
//! lost the message for good. It now reuses the boot restore's shape — `queued`
//! with the hold marker — so the message is server-side state the browser can
//! refetch, and the same Send / Cancel controls a restored row offers apply to
//! it.
//!
//! These tests pin the release half end to end (the park itself, and the
//! non-dispatch that follows it, are pinned by
//! `swallowed_send_is_retyped_then_parked_by_the_echo_deadline`): a parked row
//! is released, typed exactly once more, and matched by its echo — and a
//! release whose dialog is still up gets the same one retry an ordinary
//! dispatch would, because the park dropped the send's requeue budget.

use std::time::{Duration, Instant};

use delta_model::{SendStatus, SessionId};

use crate::error::Error;
use crate::interactor::session_actor::runtime::ECHO_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// An instant past any pending echo deadline, so a sweep fires without the
/// test ever sleeping.
fn past_deadline(from: Instant) -> Instant {
    from + ECHO_DEADLINE + Duration::from_secs(1)
}

/// How many times `text` was typed into the pane.
fn typed(ix: &TestInteractor, text: &str) -> usize {
    ix.tmux_fake()
        .sent
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, line)| line == text)
        .count()
}

#[tokio::test]
async fn parked_send_is_released_by_the_user_and_typed_once() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The message a dialog eats: nothing is ever heard about it, so both
    // deadlines fire and the second one parks it.
    let (send, _) = ix
        .enqueue_send(to(main), "swallowed by a dialog", None)
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    ix.sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    ix.sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();

    let parked = ix.store().send(send.id).await.unwrap().unwrap();
    assert_eq!(parked.status, SendStatus::Queued, "held, not cancelled");
    assert!(
        parked.held_at.is_some(),
        "the park sets the hold marker, so nothing dispatches the row on its own"
    );
    assert_eq!(
        typed(&ix, "swallowed by a dialog"),
        2,
        "dispatch + one retry"
    );

    // A further idle sweep leaves it alone: a held row is skipped by every
    // automatic dispatch trigger, exactly like a boot-restored one.
    ix.sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    assert_eq!(
        typed(&ix, "swallowed by a dialog"),
        2,
        "a parked row is never re-typed without the user asking"
    );

    // The user presses Send: the marker clears and — the session being open
    // and idle — the row types immediately through the normal queued path.
    let events = ix.release_send(send.id).await.unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SendDispatched { send_id, .. } if *send_id == send.id
        )),
        "the release reports the queued→dispatched transition; got {events:?}"
    );
    let released = ix.store().send(send.id).await.unwrap().unwrap();
    assert_eq!(released.status, SendStatus::Dispatched);
    assert!(released.held_at.is_none(), "the release cleared the hold");
    assert_eq!(
        typed(&ix, "swallowed by a dialog"),
        3,
        "the release types the message exactly one more time"
    );

    // This time the dialog is gone: the echo comes back and claims the row.
    ix.transcript_fake()
        .push(user_line("u-1", "swallowed by a dialog"));
    let (events, _) = ix
        .on_user_prompt_submit(submit("swallowed by a dialog"))
        .await
        .unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "the released send's echo correlates; it is not external input"
    );
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
    );

    // And it cannot be released twice: the marker is gone.
    assert!(matches!(
        ix.release_send(send.id).await,
        Err(Error::SendNotReleasable(id)) if id == send.id
    ));
}

/// The park drops the send's requeue budget, so a released row re-enters the
/// queue as an ordinary send: it gets its own one retry before a second park.
///
/// This only became load-bearing here. While parking cancelled the row the
/// budget could never be charged again — the send was terminal. Now the user
/// can put the same send back on the wire, and a stale budget would park it on
/// the very first deadline, with no retry at all: the release would be worth
/// strictly less than the original dispatch, for a dialog that has very likely
/// been dismissed in the meantime.
#[tokio::test]
async fn a_released_send_gets_a_fresh_retry_budget() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Dispatch, retry, park — the same two deadlines as above.
    let (send, _) = ix
        .enqueue_send(to(main), "swallowed by a dialog", None)
        .await
        .unwrap();
    ix.sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    ix.sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    assert_eq!(
        typed(&ix, "swallowed by a dialog"),
        2,
        "dispatch + one retry, then the park"
    );

    // The user releases it, and the dialog turns out to still be there.
    ix.release_send(send.id).await.unwrap();
    assert_eq!(
        typed(&ix, "swallowed by a dialog"),
        3,
        "the release types the message once"
    );
    ix.sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    assert_eq!(
        typed(&ix, "swallowed by a dialog"),
        4,
        "the released send gets its own retry rather than parking immediately"
    );
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the retry is outstanding again",
    );

    // And the fresh budget is just as bounded: the retry's own deadline parks
    // the row a second time, held for another explicit release.
    ix.sweep_echo_deadlines(past_deadline(Instant::now()))
        .await
        .unwrap();
    let reparked = ix.store().send(send.id).await.unwrap().unwrap();
    assert_eq!(reparked.status, SendStatus::Queued);
    assert!(
        reparked.held_at.is_some(),
        "the second park holds it exactly like the first"
    );
    assert_eq!(
        typed(&ix, "swallowed by a dialog"),
        4,
        "one release, one retry — never a third attempt without the user asking"
    );
}
