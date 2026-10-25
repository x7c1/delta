//! A dispatched send's `UserPromptSubmit` usually arrives before its user line
//! is in the transcript. Its turn is still announced, naming the send and its
//! thread; only the matched uuid is unknown until a later sync binds the line.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

#[tokio::test]
async fn a_send_whose_line_is_not_written_yet_announces_its_start() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The send's line is NOT in the transcript when its prompt submits.
    let (send, _) = ix.enqueue_send(to(main), "hello", None).await.unwrap();
    let (events, _) = ix.on_user_prompt_submit(submit("hello")).await.unwrap();

    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnStarted { send_id, thread_id, matched_uuid: None, .. }
                if *send_id == Some(send.id) && *thread_id == send.thread_id
        )),
        "the send's turn is announced before its line is written; got {events:?}"
    );
    let live = ix.live_state_for(&session).await;
    assert_eq!(
        live.turn,
        TurnState::InFlight {
            send_id: Some(send.id)
        }
    );
    assert_eq!(live.in_progress_thread, Some(send.thread_id));
}
