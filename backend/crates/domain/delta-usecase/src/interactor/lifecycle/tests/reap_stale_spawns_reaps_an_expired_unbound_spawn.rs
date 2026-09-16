use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::session_actor::runtime::PENDING_SPAWN_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// An unbound spawn whose deadline has passed is reaped: its pane is read and
/// killed, it is removed from the registry, its eagerly-created session row is
/// marked `failed` — kept, with everything the launch recorded — and a
/// `SpawnFailed` carrying its id, token and reason is returned.
///
/// Keeping the row is the point. A spawn that never bound ingested nothing, but
/// the row still holds the working directory, the repository, and the prompt the
/// user wrote, which is what someone opens the failed session to look at.
///
/// So is the reason. The watchdog observes silence, but silence is not "no
/// information": the launch did not bind before its deadline, and the pane is
/// still showing whatever stopped it — here, the workspace-trust dialog that
/// found this. Both go on the row, because the alternative is the screen saying
/// Delta never learned why, which is the screen a stalled launch reaches most
/// often.
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
    // What the pane is stuck on — the dialog nobody could answer.
    ix.tmux_fake().show_in_pane(
        "delta-1:0.0",
        "\n Quick safety check: Is this a project you created or one you trust?\n\n          > 1. Yes, I trust this project\n   2. No, exit\n\n",
    );

    let events = ix.reap_stale_spawns(now, TICK_BOUND).await.unwrap();

    // SpawnFailed is emitted with the minted id, the pane token, and a reason
    // built from the deadline and the pane.
    let [SessionEvent::SpawnFailed {
        session_id: failed_id,
        pane_token,
        reason,
        cancelled,
    }] = events.as_slice()
    else {
        panic!("expected one SpawnFailed, got {events:?}");
    };
    assert_eq!(failed_id, &session_id);
    assert_eq!(pane_token.as_deref(), Some("delta-1"));
    // Nobody asked for this: the spawn ran out of time.
    assert!(!cancelled);
    let reason = reason.as_deref().expect(
        "the watchdog names the deadline it enforced, rather than reporting nothing at all",
    );
    assert!(
        reason.contains("did not start within 30 seconds"),
        "the reason names the deadline the launch missed: {reason}"
    );
    assert!(
        reason.contains("Is this a project you created or one you trust?"),
        "the reason carries what the pane was showing: {reason}"
    );
    // Read while the pane was still alive: a capture after the kill would have
    // nothing to return.
    assert_eq!(
        ix.tmux_fake().captured.lock().unwrap().clone(),
        vec!["delta-1:0.0".to_owned()],
        "the pane was captured exactly once, before it was killed"
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
        session.failure_reason.as_deref(),
        Some(reason),
        "the same reason is persisted on the row, so the failed session's screen \
         shows it after a reload"
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
