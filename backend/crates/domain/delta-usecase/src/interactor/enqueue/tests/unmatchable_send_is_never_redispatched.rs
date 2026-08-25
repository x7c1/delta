//! The incident regression: a send whose echo can never match must not be
//! re-typed at all.
//!
//! A composed message went out, Claude Code submitted a prompt that did not
//! equal it, the mismatch requeued the send, the turn ended, the session went
//! idle, and the send was dispatched again — 38 times in a row, one full model
//! turn each, until a human cancelled it. A requeue budget capped that at one
//! wasted retry; deciding consumption by *position* removes the retry
//! entirely. While a send is outstanding its keystrokes are in the pane, so
//! the prompt that submits is that send's however Claude Code rewrote it: the
//! send's turn starts, and the text is never typed a second time.
//!
//! This is the net underneath any cause of a mismatch (an image attachment, a
//! folded local command, a namespaced slash command), pinning the delivery
//! count the incident got wrong.

use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};
use crate::turn::TurnState;

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
async fn unmatchable_send_is_never_redispatched() {
    let (ix, mut events) = interactor_with_event_sink();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The composed message. The prompt that comes back will differ from it,
    // standing in for whatever rewrite made the echo unmatchable.
    let (send, _) = ix.enqueue_send(to(main), "deliver me", None).await.unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    assert_eq!(typed(&ix, "deliver me"), 1, "the initial dispatch");

    // The rewritten prompt submits. It consumes the outstanding send by
    // position: the send's turn is now the in-flight one…
    ix.transcript_fake().push(user_line("u-0", "not the echo"));
    let (prompt_events, _) = ix
        .on_user_prompt_submit(submit("not the echo"))
        .await
        .unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(send.id)
        },
        "the mismatched prompt starts the outstanding send's turn",
    );
    // …so it is not announced as someone typing into the pane: it is Delta's
    // own message coming back under a different name.
    assert!(
        !prompt_events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "a consumed send's prompt is not external input; got {prompt_events:?}"
    );

    // The turn ends. The send never matched a transcript line (nothing equals
    // its text), but it WAS delivered, so it settles as `matched` — not
    // cancelled, and not left `dispatched` to shadow the next send.
    ix.on_stop(stop()).await.unwrap();
    let settled = ix.store().send(send.id).await.unwrap().expect("send row");
    assert_eq!(settled.status, SendStatus::Matched);
    assert_eq!(settled.matched_uuid, None, "delivered, but unattributed");

    // Delivered exactly once: the now-idle session has nothing left to re-type.
    // Written as a literal so any future re-type has to be a deliberate edit
    // here — and so this genuinely fails (it saw 2) against the requeue
    // behaviour it replaced.
    assert_eq!(
        typed(&ix, "deliver me"),
        1,
        "the send is typed once and never re-dispatched"
    );
    assert!(
        ix.store()
            .head_dispatched_send(&session)
            .await
            .unwrap()
            .is_none(),
        "the settled send is no longer outstanding"
    );
    let open = ix.store().open_sends(&session).await.unwrap();
    assert!(
        !open.iter().any(|s| s.id == send.id),
        "the settled send left the open-send list, so the pending chip clears"
    );

    // Nothing was parked: parking hands the text back as *undelivered*, which
    // this message no longer is.
    let mut drained = Vec::new();
    while let Ok(event) = events.try_recv() {
        drained.push(event);
    }
    assert!(
        !drained
            .iter()
            .any(|e| matches!(e, SessionEvent::SendParked { .. })),
        "a delivered message is never parked; got {drained:?}"
    );

    // A further idle does not resurrect it.
    ix.on_stop(stop()).await.unwrap();
    assert_eq!(typed(&ix, "deliver me"), 1);
}
