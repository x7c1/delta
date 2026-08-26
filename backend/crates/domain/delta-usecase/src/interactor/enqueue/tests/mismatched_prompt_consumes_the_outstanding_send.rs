//! Consumption is positional: a `UserPromptSubmit` whose text does not equal
//! the outstanding send is still that send's turn starting.
//!
//! Claude Code rewrites prompts between the keystrokes landing and the
//! submission — a namespaced slash command typed by its short name, a folded
//! local command, an unknown-command notice, the `[Image #N]` prefix — so text
//! equality cannot answer "did my send's turn start?". While a send is
//! outstanding its keystrokes are in the pane and, under the
//! single-outstanding rule, nothing else of Delta's is: the prompt that
//! submits is that send's, whatever it says.

use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

#[tokio::test]
async fn mismatched_prompt_consumes_the_outstanding_send() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Dispatched and awaiting its echo.
    let (send, _) = ix
        .enqueue_send(to(main), "/plan the migration", None)
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id },
    );

    // Claude Code records something else entirely for it.
    ix.transcript_fake()
        .push(user_line("u-1", "<command-name>/plan</command-name>"));
    let (events, _) = ix
        .on_user_prompt_submit(submit("<command-name>/plan</command-name>"))
        .await
        .unwrap();

    // The send's turn is the one now running: no orphan, and nothing is
    // re-typed. The transcript line ingested by the same sync consumes the row
    // by position too, so it settles as `matched` against that line rather than
    // being requeued or cancelled.
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(send.id)
        },
    );
    let row = ix.store().send(send.id).await.unwrap().unwrap();
    assert_eq!(
        row.status,
        SendStatus::Matched,
        "the send is consumed, not requeued and not cancelled",
    );
    assert_eq!(
        row.matched_uuid.as_ref().map(|u| u.as_str()),
        Some("u-1"),
        "the line that consumed it is the line it is bound to",
    );
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "the message is typed once: a rewritten echo never re-dispatches it",
    );

    // And it is not reported as pane typing: Delta believes this prompt is its
    // own send, so calling it external would contradict that same decision.
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "got {events:?}"
    );
    // The hook's own announcement still keys on the text: no submitted prompt
    // equals the send, so no `TurnStarted` goes out here (the `TurnCompleted`
    // refetch covers the UI). The line's THREAD attribution no longer depends
    // on that — the ingest above already bound it to the send.
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::TurnStarted { .. })),
        "an unattributed send announces no matched uuid; got {events:?}"
    );
}
