use std::time::{Duration, Instant};

use delta_model::SessionId;

use crate::interactor::testing::*;

/// A freshly-recorded spawn whose deadline has not yet passed is NOT reaped:
/// no pane is killed, no event is produced, and it stays pending so the normal
/// `UserPromptSubmit` bind path can still claim it.
#[tokio::test]
async fn reap_stale_spawns_leaves_a_fresh_spawn_alone() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-young");

    // Seed a spawn stamped just one second ago — far inside its deadline.
    ix.push_pending_spawn_at("delta-1", &session_id, now - Duration::from_secs(1))
        .await;
    ix.tmux_fake()
        .live
        .lock()
        .unwrap()
        .push("delta-1".to_owned());

    let events = ix.reap_stale_spawns(now).await.unwrap();

    assert!(events.is_empty(), "a fresh spawn yields no SpawnFailed");
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "a fresh spawn's pane is not killed"
    );
    // Still pending, still bindable by its id.
    assert_eq!(ix.pending_session_ids().await, vec![session_id]);
}
