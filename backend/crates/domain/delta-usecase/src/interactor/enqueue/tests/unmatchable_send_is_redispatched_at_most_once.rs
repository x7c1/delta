//! The incident regression: a send whose echo can never match must not be
//! re-typed forever.
//!
//! A composed message went out, Claude Code submitted a prompt that did not
//! equal it, the mismatch requeued the send, the turn ended, the session went
//! idle, and the send was dispatched again — 38 times in a row, one full model
//! turn each, until a human cancelled it. The cause of that particular
//! mismatch (an image attachment) is fixed separately in the echo matching;
//! this is the net underneath it, which must hold for *any* cause.

use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

/// A `Stop` for the seeded session: ends the turn, returning it to idle so any
/// queued send dispatches.
fn stop() -> StopHook {
    StopHook {
        session_id: SessionId::from("sess-1"),
        stop_reason: None,
    }
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
async fn unmatchable_send_is_redispatched_at_most_once() {
    let (ix, mut events) = interactor_with_event_sink();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The composed message. Every prompt that comes back will differ from it,
    // standing in for whatever rewrite/mangling made the echo unmatchable.
    let (send, _) = ix.enqueue_send(to(main), "deliver me", None).await.unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    assert_eq!(typed(&ix, "deliver me"), 1, "the initial dispatch");

    // Two full mismatch → turn-end → idle cycles. The first requeues and
    // re-dispatches (today's behaviour, deliberately preserved); the second
    // finds the budget spent.
    for round in 0..2 {
        ix.transcript_fake()
            .push(user_line(&format!("u-{round}"), "not the echo"));
        let (prompt_events, _) = ix
            .on_user_prompt_submit(submit("not the echo"))
            .await
            .unwrap();
        // The budget changes only what happens to the SEND. The prompt that
        // arrived is someone typing into the pane either way, so it is still
        // announced as external input on the round that parks the send just
        // as on the round that requeues it.
        assert!(
            prompt_events
                .iter()
                .any(|e| matches!(e, SessionEvent::ExternalInput { prompt, .. }
                    if prompt == "not the echo")),
            "round {round}: the mismatched prompt is still external input; \
             got {prompt_events:?}"
        );
        ix.on_stop(stop()).await.unwrap();
    }

    // One retry, and no more: the loop is finite. Written as a literal rather
    // than derived from the cap, so raising the cap has to be a deliberate
    // edit here too — and so this assertion genuinely fails (it saw 3) against
    // the unbounded behaviour it was written for.
    assert_eq!(
        typed(&ix, "deliver me"),
        2,
        "the send is dispatched once and re-dispatched exactly once"
    );

    // The send has left the dispatch queue entirely — parked, not silently
    // dropped and not lingering as a `queued` row a later idle could pick up.
    assert!(
        ix.store()
            .head_dispatched_send(&session)
            .await
            .unwrap()
            .is_none(),
        "the parked send is no longer outstanding"
    );
    let open = ix.store().open_sends(&session).await.unwrap();
    assert!(
        !open.iter().any(|s| s.id == send.id),
        "the parked send left the open-send list, so the pending chip clears"
    );
    let parked_row = ix.store().send(send.id).await.unwrap().expect("send row");
    assert_eq!(parked_row.status, SendStatus::Cancelled);

    // …and the browser is told why, with the text it can hand back to the user.
    let mut drained = Vec::new();
    while let Ok(event) = events.try_recv() {
        drained.push(event);
    }
    assert!(
        drained.contains(&SessionEvent::SendParked {
            session_id: session.clone(),
            send_id: send.id,
            text: "deliver me".to_owned(),
        }),
        "the park is announced, never a silent drop; got {drained:?}"
    );

    // A further idle does not resurrect it.
    ix.on_stop(stop()).await.unwrap();
    assert_eq!(typed(&ix, "deliver me"), 2);
}
