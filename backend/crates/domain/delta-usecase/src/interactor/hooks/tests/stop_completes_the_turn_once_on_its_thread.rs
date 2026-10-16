use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

/// A `Stop` hook ends the in-flight turn: it reports exactly one
/// `TurnCompleted`, naming the session and the thread whose turn ran, and
/// returns the turn machine to `Idle`.
#[tokio::test]
async fn stop_completes_the_turn_once_on_its_thread() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    ix.enqueue_send(to(main), "hello", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-1", "hello"));
    ix.on_user_prompt_submit(submit("hello")).await.unwrap();

    let events = ix
        .on_stop(crate::ports::StopHook {
            session_id: session.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();

    let completions: Vec<&SessionEvent> = events
        .iter()
        .filter(|e| matches!(e, SessionEvent::TurnCompleted { .. }))
        .collect();
    assert_eq!(
        completions,
        [&SessionEvent::TurnCompleted {
            session_id: session.clone(),
            thread_id: Some(main),
            stop_reason: None,
        }],
        "one Stop reports exactly one TurnCompleted, on the thread that ran"
    );
    assert_eq!(ix.live_state_for(&session).await.turn, TurnState::Idle);
}
