use std::time::{Duration, Instant};

use delta_model::SessionId;

use super::listed_state;
use crate::interactor::session_actor::runtime::PENDING_SPAWN_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// The failed twin of [`list_sessions_page_reports_a_starting_pane_until_it_binds`]:
/// a spawn the watchdog gave up on had its pane killed with it, so its kept row
/// reports no starting pane — a session whose terminal would refuse to attach
/// must not look attachable.
///
/// [`list_sessions_page_reports_a_starting_pane_until_it_binds`]: super::list_sessions_page_reports_a_starting_pane_until_it_binds
#[tokio::test]
async fn list_sessions_page_reports_no_starting_pane_for_a_failed_launch() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-doomed");

    ix.store()
        .insert_spawning_session(spawning_session(&session_id, "/work"))
        .await
        .unwrap();
    ix.push_pending_spawn_at(
        "delta-1",
        &session_id,
        now - PENDING_SPAWN_DEADLINE - Duration::from_secs(1),
    )
    .await;
    ix.tmux_fake()
        .live
        .lock()
        .unwrap()
        .push("delta-1".to_owned());

    assert_eq!(
        listed_state(&ix, &session_id).await,
        (false, true),
        "attachable while the pane is still alive"
    );

    let events = ix.reap_stale_spawns(now, TICK_BOUND).await.unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionEvent::SpawnFailed { .. })),
        "the sweep reaped the spawn"
    );

    assert_eq!(
        listed_state(&ix, &session_id).await,
        (false, false),
        "a failed launch's pane is gone with it"
    );
}
