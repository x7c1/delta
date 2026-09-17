//! A resume binds its pane immediately but is not ready until
//! `SessionStart(source=resume)` arrives, and that window belongs to the resume
//! reaper and its deadline — which reports a *failed resume* (`SpawnFailed`,
//! held prompt cancelled) rather than a close. The pane probe must therefore
//! keep its hands off a resuming session, even though its pane is not yet
//! something tmux reports: a resume caught mid-launch would otherwise be closed
//! out from under the reaper that owns it.
//!
//! What happens to a resume that runs out its deadline is
//! `reap_stale_resuming_fails_a_resume_that_never_became_ready`.

use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;

#[tokio::test]
async fn a_vanished_pane_leaves_a_resuming_session_to_the_resume_reaper() {
    let ix = interactor();
    let now = Instant::now();
    let session_id = SessionId::from("sess-resuming");

    ix.on_user_prompt_submit(submit_in(
        session_id.as_str(),
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    // Bound but not ready, and well inside its readiness deadline. Its pane is
    // not in tmux's session list, so nothing but the resuming guard is keeping
    // the probe off it.
    ix.push_resuming_at("delta-7", &session_id, Some("held".to_owned()), now)
        .await;

    let events = ix.reap_stale_spawns(now, TICK_BOUND).await.unwrap();

    assert!(
        events.is_empty(),
        "a resuming session produces no close, got {events:?}"
    );
    assert_eq!(
        ix.resuming_session_ids().await,
        vec![session_id.clone()],
        "it is still resuming, awaiting its readiness hook or its deadline"
    );
    assert!(
        ix.is_session_open(&session_id).await,
        "its binding is untouched"
    );
}
