//! A prompt typed straight into the pane consumes no send. It is reported as
//! external input AND its turn is announced, so the session list shows it
//! running like a turn Delta started.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

#[tokio::test]
async fn a_prompt_typed_into_the_pane_announces_its_start() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    ix.transcript_fake()
        .push(user_line("u-1", "typed in the pane"));
    let (events, _) = ix
        .on_user_prompt_submit(submit("typed in the pane"))
        .await
        .unwrap();

    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "got {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnStarted { send_id: None, matched_uuid: None, thread_id, .. }
                if *thread_id == main
        )),
        "the typed prompt's turn is announced on main; got {events:?}"
    );
    let live = ix.live_state_for(&session).await;
    assert_eq!(live.turn, TurnState::InFlight { send_id: None });
    assert_eq!(live.in_progress_thread, Some(main));
}
