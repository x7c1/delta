use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEndHook, SessionEvent};

/// A `SessionEnd` for a session that is resumed but not yet ready is a failed
/// resume: the resuming entry is dropped, its pane killed, its held first prompt
/// cancelled, and a `SpawnFailed` emitted — the precise early counterpart to the
/// resume watchdog deadline.
#[tokio::test]
async fn session_end_for_resuming_session_reports_failed() {
    let ix = interactor();
    let session_id = SessionId::from("sess-resume-end");

    // Register the session and write its held first prompt, then mark it
    // resuming-but-not-ready with a live pane.
    ix.on_user_prompt_submit(submit_in(
        session_id.as_str(),
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    let held = ix
        .store()
        .enqueue_send(&session_id, main, None, "held prompt", None)
        .await
        .unwrap();
    // The held prompt counts as dispatched (its row is out awaiting its echo),
    // exactly as `enqueue_into_open` records it.
    ix.apply_turn_input(
        &session_id,
        crate::turn::TurnInput::Dispatch { send_id: held.id },
    )
    .await
    .unwrap();
    ix.push_resuming_at(
        "delta-3",
        &session_id,
        Some("held prompt".to_owned()),
        Instant::now(),
    )
    .await;
    ix.tmux_fake()
        .live
        .lock()
        .unwrap()
        .push("delta-3".to_owned());

    let events = ix
        .on_session_end(SessionEndHook {
            session_id: session_id.clone(),
            reason: Some("exit".into()),
        })
        .await
        .unwrap();

    assert_eq!(
        events,
        vec![SessionEvent::SpawnFailed {
            session_id: session_id.clone(),
            pane_token: Some("delta-3".to_owned()),
            // The hook reports only that the launch ended, never why.
            reason: None,
        }],
    );
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-3".to_owned()],
    );
    assert!(
        ix.store()
            .head_dispatched_send(&session_id)
            .await
            .unwrap()
            .is_none(),
        "the held prompt's pending row was cancelled"
    );
    assert!(ix.resuming_session_ids().await.is_empty());
}
