use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::StopHook;

/// Single-outstanding dispatch: at most one send is out per turn, so a plain
/// main-line send issued mid-turn is held `queued` — exactly like a branch or
/// quoted send — and dispatched when the turn ends. (Previously plain sends
/// were typed into the busy pane for Claude Code to queue itself, which made
/// `UserPromptSubmit` correlation a FIFO text scan over several dispatched
/// rows; holding everything server-side keeps a single outstanding send.)
#[tokio::test]
async fn plain_send_mid_turn_is_queued() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Start a turn: dispatch and echo the first send.
    ix.enqueue_send(to(main), "first", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-first", "first"));
    ix.on_user_prompt_submit(submit("first")).await.unwrap();
    assert_ne!(ix.turn_state_for(&session).await, crate::turn::TurnState::Idle);

    // A plain main-line send mid-turn is held queued, not typed into the pane.
    let (second, _) = ix.enqueue_send(to(main), "second", None).await.unwrap();
    assert_eq!(second.status, SendStatus::Queued);
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "no keystrokes are dispatched mid-turn"
    );

    // The turn ends: the queued send dispatches as the next outstanding send.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    let (count, dispatched) = {
        let sent = ix.tmux_fake().sent.lock().unwrap();
        (sent.len(), sent.last().map(|p| p.1.clone()))
    };
    assert_eq!(count, 2, "the queued send dispatched at turn end");
    assert_eq!(dispatched.as_deref(), Some("second"));
    assert!(ix
        .store()
        .next_queued_send(&session)
        .await
        .unwrap()
        .is_none());
}
