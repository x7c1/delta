use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::session_actor::runtime::PENDING_SPAWN_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// An unbound spawn whose deadline has passed is reaped: its pane is killed, it
/// is removed from the registry, its eagerly-created session row is marked
/// `failed` — kept, with everything the launch recorded — and a `SpawnFailed`
/// carrying its id and token is returned.
///
/// Keeping the row is the point. A spawn that never bound ingested nothing, but
/// the row still holds the working directory, the repository, and the prompt the
/// user wrote, which is what someone opens the failed session to look at.
#[tokio::test]
async fn reap_stale_spawns_reaps_an_expired_unbound_spawn() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-stuck");

    // Seed the eager `spawning` row a real spawn would have written — with the
    // launch context a real one records — plus the first prompt it accepted,
    // then a spawn stamped one second past its deadline, with a live tmux
    // session so the reaper actually issues (and we can observe) the kill.
    let (_, main) = ix
        .store()
        .insert_spawning_session(crate::SpawningSession {
            repository_display_name: Some("x7c1/delta"),
            ..spawning_session(&session_id, "/work")
        })
        .await
        .unwrap();
    ix.store()
        .enqueue_queued_send(
            &session_id,
            main,
            None,
            "the prompt that never went out",
            None,
        )
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

    let events = ix.reap_stale_spawns(now, TICK_BOUND).await.unwrap();

    // SpawnFailed is emitted with the minted id and the pane token.
    assert_eq!(
        events,
        vec![SessionEvent::SpawnFailed {
            session_id: session_id.clone(),
            pane_token: Some("delta-1".to_owned()),
            // The watchdog observes silence, so it names no cause.
            reason: None,
            // Nobody asked for this: the spawn ran out of time.
            cancelled: false,
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
    // The eager session row is kept and marked `failed`, with everything the
    // launch recorded still on it.
    let session = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the never-bound spawn's session row is kept at reap time");
    assert_eq!(session.status, delta_model::SessionStatus::Failed);
    assert_eq!(session.cwd, "/work", "the workdir survives the reap");
    assert_eq!(
        session.repository_display_name.as_deref(),
        Some("x7c1/delta"),
        "the repository survives the reap"
    );
    assert_eq!(
        session.failure_reason, None,
        "the watchdog observes only silence, so it records no cause"
    );

    // And the prompt the user wrote is still open against the row — nothing
    // cascaded away, because nothing was deleted.
    assert_eq!(
        ix.store()
            .open_sends(&session_id)
            .await
            .unwrap()
            .into_iter()
            .map(|send| send.text)
            .collect::<Vec<_>>(),
        vec!["the prompt that never went out".to_owned()],
    );
}
