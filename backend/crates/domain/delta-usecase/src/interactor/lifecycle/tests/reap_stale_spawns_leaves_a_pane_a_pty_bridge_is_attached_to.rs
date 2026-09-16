use std::time::{Duration, Instant};

use delta_model::SessionId;

use crate::interactor::session_actor::runtime::PENDING_SPAWN_DEADLINE;
use crate::interactor::testing::*;

/// The watchdog does not kill a pane somebody is attached to.
///
/// The deadline is for a launch nobody can reach — it crashed, or it stopped on
/// a prompt no hook will ever answer. A browser holding a PTY bridge on the pane
/// is the answer to precisely that second case: the user is looking at the
/// dialog and can press the key that lets the launch continue. Reaping then
/// would kill the pane mid-answer, which is what dogfooding hit twice in a row.
///
/// Attaching only *defers* the reap: the spawn stays pending, and once the last
/// bridge is gone the deadline is enforced again — measured from that detach,
/// so the user who steps away gets the full grace over again.
#[tokio::test]
async fn reap_stale_spawns_leaves_a_pane_a_pty_bridge_is_attached_to() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-watched");

    ix.store()
        .insert_spawning_session(spawning_session(&session_id, "/work"))
        .await
        .unwrap();
    // A spawn a second past its deadline, with a live pane for the reaper to
    // kill — everything the sweep needs to fire.
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

    // The browser attaches to the still-starting session's pane.
    let attached = ix.attach_pane(&session_id).await;
    assert!(
        attached.is_some_and(|pane| !pane.bound),
        "the bridge attached to an unbound spawn's pane"
    );

    let events = ix.reap_stale_spawns(now, TICK_BOUND).await.unwrap();

    assert!(
        events.is_empty(),
        "a pane being watched is not reported as a failed launch"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "the pane the user is typing into is not killed"
    );
    assert_eq!(
        ix.pending_session_ids().await,
        vec![session_id.clone()],
        "the spawn is still pending, so its first hook can still bind it"
    );

    // A second tab attaches to the same pane, then the first one goes. The
    // record is a count, not a flag, precisely for this: one bridge leaving
    // while another is still there must not hand the pane back to the reaper,
    // or the tab still on screen loses the dialog it was answering.
    let second = ix.attach_pane(&session_id).await;
    assert!(
        second.is_some(),
        "a second bridge attaches to the same pane"
    );
    ix.detach_pane(&session_id, now).await;
    let events = ix.reap_stale_spawns(now, TICK_BOUND).await.unwrap();

    assert!(
        events.is_empty(),
        "one of two bridges leaving still leaves somebody watching the pane"
    );
    assert_eq!(
        ix.pending_session_ids().await,
        vec![session_id.clone()],
        "so the spawn is still pending"
    );

    // The last browser goes away, and the deadline is enforced again — from the
    // detach, not from the launch: leaving restarts the clock, so the sweep that
    // reaps this spawn is a whole deadline later (that grace has its own test,
    // `detaching_from_a_pane_gives_the_launch_its_deadline_afresh`).
    ix.detach_pane(&session_id, now).await;
    let events = ix
        .reap_stale_spawns(now + PENDING_SPAWN_DEADLINE, TICK_BOUND)
        .await
        .unwrap();

    assert_eq!(events.len(), 1, "with nobody attached, the spawn is reaped");
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-1".to_owned()],
        "and its pane is killed"
    );
}
