use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::open_sessions::RESUME_READY_DEADLINE;

/// A resume that became ready in time is not failed by the watchdog. Even past
/// the deadline instant, once `SessionStart(source=resume)` has marked it ready
/// it is no longer in the resuming set, so the reaper never touches it.
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
    // It stayed bound (open) and left the resuming set when it became ready.
    assert!(ix.is_session_open(&session_id).await);
    assert!(ix.resuming_session_ids().await.is_empty());
}
