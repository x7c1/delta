//! A background task (a subagent, a `run_in_background` Bash) that finishes
//! after the turn ended makes the harness submit a `<task-notification>`
//! prompt, which starts a new turn while no send is outstanding. That turn is
//! announced like any other, so a session that is not open shows it running.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};
use crate::turn::TurnState;

#[tokio::test]
async fn a_task_notification_turn_after_stop_announces_its_start() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A send's turn runs and ends.
    ix.enqueue_send(to(main), "hello", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-1", "hello"));
    ix.on_user_prompt_submit(submit("hello")).await.unwrap();
    ix.transcript_fake()
        .push(assistant_line("a-1", "waiting for the agent"));
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(ix.live_state_for(&session).await.turn, TurnState::Idle);

    // The background task completes and the harness starts a new turn.
    let prompt = "<task-notification>\n\
                  <task-id>a31425032172620ed</task-id>\n\
                  <tool-use-id>toolu_bg</tool-use-id>\n\
                  <output-file>/tmp/x.output</output-file>\n\
                  <status>completed</status>\n\
                  <summary>Agent completed</summary>\n\
                  </task-notification>";
    ix.transcript_fake()
        .push(task_notification_line("u-2", "toolu_bg"));
    let (events, _) = ix.on_user_prompt_submit(submit(prompt)).await.unwrap();

    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnStarted { send_id: None, matched_uuid: None, thread_id, .. }
                if *thread_id == main
        )),
        "the notification's turn is announced on main; got {events:?}"
    );
    let live = ix.live_state_for(&session).await;
    assert_eq!(live.turn, TurnState::InFlight { send_id: None });
    assert_eq!(
        live.in_progress_thread,
        Some(main),
        "the live state names the thread the announcement lit"
    );
}
