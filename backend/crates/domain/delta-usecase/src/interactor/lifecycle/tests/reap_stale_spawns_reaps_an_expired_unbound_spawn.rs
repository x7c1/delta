use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::session_actor::runtime::PENDING_SPAWN_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// An unbound spawn whose deadline has passed is reaped: its pane is killed, it
/// is removed from the registry, its eagerly-created `spawning` session row
/// (which ingested nothing) is deleted, and a `SpawnFailed` carrying its id and
/// token is returned.
#[tokio::test]
async fn reap_stale_spawns_reaps_an_expired_unbound_spawn() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-stuck");

    // Seed the eager `spawning` row a real spawn would have written, then a
    // spawn stamped one second past its deadline, with a live tmux session so
    // the reaper actually issues (and we can observe) the kill.
    ix.store()
        .insert_spawning_session(spawning_session(&session_id, "/work"))
        .await
        .unwrap();
    ix.push_pending_spawn_at(
        "delta-1",
        &session_id,
        now - PENDING_SPAWN_DEADLINE - std::time::Duration::from_secs(1),
    )
    .await;
    ix.tmux_fake()
        .live
        .lock()
        .unwrap()
        .push("delta-1".to_owned());

    let events = ix.reap_stale_spawns(now).await.unwrap();

    // SpawnFailed is emitted with the minted id and the pane token.
    assert_eq!(
        events,
        vec![SessionEvent::SpawnFailed {
            session_id: session_id.clone(),
            pane_token: Some("delta-1".to_owned()),
            // The watchdog observes silence, so it names no cause.
            reason: None,
            // The reaped spawn was seeded through the runtime seam and accepted
            // no send, so it has no undelivered text to hand back.
            unsent: Vec::new(),
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
    // The eager session row ingested nothing, so the reap deleted it (and its
    // children, by cascade) rather than leaving a dead `spawning` row behind.
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the never-bound spawn's session row is deleted at reap time"
    );
}
