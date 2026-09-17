//! The guard that matters most: a healthy open session must survive every
//! tick, however long it has been open. There is deliberately no age limit on
//! the pane probe — only the pane's absence closes a session.

use std::time::{Duration, Instant};

use crate::interactor::testing::*;

#[tokio::test]
async fn a_live_pane_keeps_its_session_open_on_the_tick() {
    let ix = interactor();

    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(ix.is_session_open(&id).await, "open on a live pane");

    // A `now` far past every launch deadline, so nothing but the pane's own
    // presence is keeping this session open.
    let events = ix
        .reap_stale_spawns(Instant::now() + Duration::from_secs(3600), TICK_BOUND)
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "a live pane produces no events, got {events:?}"
    );
    assert!(
        ix.is_session_open(&id).await,
        "the session is still open after the tick"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "no pane is torn down"
    );
}
