use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::open_sessions::PENDING_SPAWN_DEADLINE;
use crate::ports::SessionEvent;

/// An unbound spawn whose deadline has passed is reaped: its pane is killed, it
/// is removed from the registry, and a `SpawnFailed` carrying its id and token
/// is returned.
#[tokio::test]
async fn reap_stale_spawns_reaps_an_expired_unbound_spawn() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-stuck");

    // Seed a spawn stamped one second past its deadline, with a live tmux
    // session so the reaper actually issues (and we can observe) the kill.
    ix.push_pending_spawn_at(
        "delta-1",
        &session_id,
        now - PENDING_SPAWN_DEADLINE - std::time::Duration::from_secs(1),
    )
    .await;
    ix.tmux_fake().live.lock().unwrap().push("delta-1".to_owned());

    let events = ix.reap_stale_spawns(now).await.unwrap();

    // SpawnFailed is emitted with the minted id and the pane token.
    assert_eq!(
        events,
        vec![SessionEvent::SpawnFailed {
            session_id: session_id.clone(),
            pane_token: "delta-1".to_owned(),
        }],
    );
    // The pane was killed by token.
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-1".to_owned()],
    );
    // The spawn is gone from the registry: a later UserPromptSubmit for that id
    // can no longer bind it.
    assert!(ix.pending_session_ids().await.is_empty());
}
