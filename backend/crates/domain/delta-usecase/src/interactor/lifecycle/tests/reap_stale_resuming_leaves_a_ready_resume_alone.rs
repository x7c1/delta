use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::open_sessions::RESUME_READY_DEADLINE;

/// A resume that became ready in time is not failed by the watchdog. Even past
/// the read-deadline instant, once `SessionStart(source=resume)` has stamped its
/// `ready_at` it is pending dispatch, not stalled, so the reaper must skip it —
/// even though it is still in the resuming map (it leaves the map only when the
/// dispatch tick types its held prompt).
#[tokio::test]
async fn reap_stale_resuming_leaves_a_ready_resume_alone() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-ready");

    // Seed a resuming entry that is already past its deadline...
    ix.push_resuming_at(
        "delta-9",
        &session_id,
        None,
        now - RESUME_READY_DEADLINE - std::time::Duration::from_secs(1),
    )
    .await;
    // ...but it became ready (its readiness hook fired) before this sweep.
    ix.on_session_start(session_start(session_id.as_str(), "resume"))
        .await
        .unwrap();

    let events = ix.reap_stale_spawns(now).await.unwrap();

    assert!(events.is_empty(), "a ready resume is not failed");
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "a ready resume's pane is not killed"
    );
    // It stayed bound (open) and stayed in the resuming map — `ready_at = Some`
    // means pending dispatch, which the watchdog must leave for the dispatch tick.
    assert!(ix.is_session_open(&session_id).await);
    assert_eq!(
        ix.resuming_session_ids().await,
        vec![session_id.clone()],
        "a ready-but-not-yet-dispatched resume stays in the resuming map"
    );
}
