//! A session that holds no pane handle has nothing to probe: a known session
//! that was never opened (an external `claude` registered by its hook) and one
//! that has already been closed both sit out the tick entirely. Closing a
//! closed session again would emit a second `session_closed` for a state
//! nothing changed.

use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;

#[tokio::test]
async fn the_pane_probe_ignores_a_session_that_is_not_open() {
    let ix = interactor();
    let never_opened = SessionId::from("sess-external");
    ix.on_user_prompt_submit(submit_in(
        never_opened.as_str(),
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    assert!(
        !ix.is_session_open(&never_opened).await,
        "known but never opened by Delta"
    );

    // A second session that was open and has since been closed explicitly.
    let closed = SessionId::from("sess-1");
    ix.seed_session().await;
    ix.close_session(&closed).await.unwrap();
    assert!(!ix.is_session_open(&closed).await, "closed");
    ix.tmux_fake().killed.lock().unwrap().clear();

    let events = ix
        .reap_stale_spawns(Instant::now(), TICK_BOUND)
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "neither session produces an event, got {events:?}"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "nothing is torn down"
    );
}
