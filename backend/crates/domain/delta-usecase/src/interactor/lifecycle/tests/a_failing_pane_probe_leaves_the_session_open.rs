//! tmux itself failing is not "the pane is gone". Only a definite "no such
//! session" closes a session — closing a healthy one because a `has-session`
//! call errored would be far worse than waiting for the next tick.

use std::time::Instant;

use crate::interactor::testing::*;

#[tokio::test]
async fn a_failing_pane_probe_leaves_the_session_open() {
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

    // tmux stops answering. The pane is still there; Delta just cannot see it.
    ix.tmux_fake().fail_probes();

    let events = ix
        .reap_stale_spawns(Instant::now(), TICK_BOUND)
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "an unreadable probe reports nothing, got {events:?}"
    );
    assert!(
        ix.is_session_open(&id).await,
        "the session stays open when the probe fails"
    );
}
