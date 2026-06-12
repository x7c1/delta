use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::open_sessions::RESUME_READY_DEADLINE;
use crate::ports::SessionEvent;

/// A resumed session that never became ready before its deadline is failed by
/// the watchdog: its pane is killed, its held first prompt is cancelled, it is
/// removed from the registry, and a `SpawnFailed` carrying its id and token is
/// returned.
#[tokio::test]
async fn reap_stale_resuming_fails_a_resume_that_never_became_ready() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-stuck-resume");

    // Register the session so a send (the held first prompt) can be
    // written against it, then seed a resuming entry one second past its
    // deadline with a held prompt and a live tmux pane.
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
        "delta-7",
        &session_id,
        Some("held prompt".to_owned()),
        now - RESUME_READY_DEADLINE - std::time::Duration::from_secs(1),
    )
    .await;
    ix.tmux_fake().live.lock().unwrap().push("delta-7".to_owned());

    let events = ix.reap_stale_spawns(now).await.unwrap();

    // SpawnFailed is emitted with the resumed session's id and pane token.
    assert_eq!(
        events,
        vec![SessionEvent::SpawnFailed {
            session_id: session_id.clone(),
            pane_token: "delta-7".to_owned(),
        }],
    );
    // The pane was killed.
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-7".to_owned()],
    );
    // The held first prompt was cancelled so it cannot block a later re-resume.
    assert!(
        ix.store()
            .head_dispatched_send(&session_id)
            .await
            .unwrap()
            .is_none(),
        "the held prompt's pending row was cancelled"
    );
    // The session is no longer resuming or open.
    assert!(ix.resuming_session_ids().await.is_empty());
    assert!(!ix.is_session_open(&session_id).await);
}
