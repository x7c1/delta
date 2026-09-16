use std::time::{Duration, Instant};

use delta_model::SessionId;

use crate::interactor::session_actor::runtime::PENDING_SPAWN_DEADLINE;
use crate::interactor::testing::*;

/// Closing the terminal does not kill the session it was showing.
///
/// The watchdog will not reap a pane a PTY bridge is attached to, but the bind
/// deadline keeps running underneath it. So a user who opened the terminal on a
/// still-starting session, read the first-run dialog for most of a minute and
/// then closed the terminal column used to have the session reaped on the very
/// next sweep — which reads as "closing the terminal killed it", and nothing on
/// screen says closing has any such effect.
///
/// The last bridge leaving therefore restarts the clock: somebody who steps
/// away from a pane gets the same grace as a pane nobody ever attached to.
#[tokio::test]
async fn detaching_from_a_pane_gives_the_launch_its_deadline_afresh() {
    let ix = interactor();
    let launched_at = Instant::now();
    // Long enough that the original deadline would have expired while the user
    // was reading, which is the whole of the case.
    let detached_at = launched_at + Duration::from_secs(20);
    let session_id = SessionId::from("sess-read-then-closed");

    ix.store()
        .insert_spawning_session(spawning_session(&session_id, "/work"))
        .await
        .unwrap();
    // A fresh spawn with a live pane: the launch is up and waiting on its first
    // hook, which is exactly when a browser may reach it.
    ix.push_pending_spawn_at("delta-1", &session_id, launched_at)
        .await;
    ix.tmux_fake()
        .live
        .lock()
        .unwrap()
        .push("delta-1".to_owned());

    // The user opens the terminal, reads the dialog, and closes the column.
    assert!(
        ix.attach_pane(&session_id).await.is_some(),
        "the bridge attached to the still-starting session's pane"
    );
    ix.detach_pane(&session_id, detached_at).await;

    // The instant the ORIGINAL deadline would have run out. Nothing may happen
    // here: the sweep that fires a moment after the user closes the terminal is
    // precisely the one that looked like the close killing the session.
    let events = ix
        .reap_stale_spawns(launched_at + PENDING_SPAWN_DEADLINE, TICK_BOUND)
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "the launch the user just stopped watching is not reported as failed"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "and its pane is left alone"
    );
    assert_eq!(
        ix.pending_session_ids().await,
        vec![session_id.clone()],
        "the spawn is still pending, so its first hook can still bind it"
    );

    // A full deadline after the detach, though, it is as stuck as any launch
    // nobody ever reached — the grace is a restart, not an exemption.
    let events = ix
        .reap_stale_spawns(detached_at + PENDING_SPAWN_DEADLINE, TICK_BOUND)
        .await
        .unwrap();

    assert_eq!(
        events.len(),
        1,
        "a deadline after the detach, the spawn is reaped"
    );
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-1".to_owned()],
        "and its pane is killed"
    );
}
