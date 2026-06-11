use std::time::Instant;

use crate::interactor::testing::*;

/// A spawn that bound before the deadline is NOT reaped: once a
/// `UserPromptSubmit` moves it `pending → bound`, it is no longer in the
/// pending set the reaper scans, so even a long-since-elapsed `now` leaves the
/// live session and its pane untouched.
#[tokio::test]
async fn reap_stale_spawns_leaves_a_bound_session_alone() {
    let ix = interactor();

    // Spawn and bind a session through the normal path.
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
    assert!(ix.pane_for_session(&id).await.is_some(), "bound and open");

    // Reap with a `now` far past any deadline: the bound session is not pending,
    // so nothing is reaped.
    let events = ix
        .reap_stale_spawns(Instant::now() + std::time::Duration::from_secs(3600))
        .await
        .unwrap();

    assert!(events.is_empty(), "a bound session yields no SpawnFailed");
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "a bound session's pane is not killed by the reaper"
    );
    assert!(
        ix.pane_for_session(&id).await.is_some(),
        "the session is still open after the sweep"
    );
}
