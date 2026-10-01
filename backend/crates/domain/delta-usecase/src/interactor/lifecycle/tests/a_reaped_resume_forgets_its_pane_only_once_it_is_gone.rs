//! A resume the watchdog gives up on forgets the pane its row remembers only
//! when that pane is confirmed gone. A pane whose kill could not be confirmed
//! may still be running, and the record is what lets the next send adopt it
//! instead of launching a second agent beside it.

use std::time::{Duration, Instant};

use delta_model::SessionId;

use crate::interactor::session_actor::runtime::RESUME_READY_DEADLINE;
use crate::interactor::testing::*;

/// Spawn, bind and close a session, then resume it, leaving a resume that
/// never becomes ready and whose pane the row remembers.
async fn stalled_resume(ix: &TestInteractor) -> SessionId {
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
    ix.close_session(&id).await.unwrap();
    ix.open_session(&id).await.unwrap();
    assert!(ix.store().remembered_pane(&id).await.unwrap().is_some());
    id
}

fn past_the_resume_deadline() -> Instant {
    Instant::now() + RESUME_READY_DEADLINE + Duration::from_secs(1)
}

#[tokio::test]
async fn a_reaped_resume_forgets_the_pane_it_killed() {
    let ix = interactor();
    let id = stalled_resume(&ix).await;

    ix.reap_stale_spawns(past_the_resume_deadline(), TICK_BOUND)
        .await
        .unwrap();

    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().last().cloned(),
        Some("delta-2".into())
    );
    assert_eq!(ix.store().remembered_pane(&id).await.unwrap(), None);
}

#[tokio::test]
async fn a_reaped_resume_keeps_the_record_of_a_pane_it_could_not_confirm_gone() {
    let ix = interactor();
    let id = stalled_resume(&ix).await;
    ix.tmux_fake().fail_probes();

    ix.reap_stale_spawns(past_the_resume_deadline(), TICK_BOUND)
        .await
        .unwrap();

    assert!(!ix.is_session_open(&id).await);
    assert_eq!(
        ix.store()
            .remembered_pane(&id)
            .await
            .unwrap()
            .map(|p| p.tmux_session),
        Some("delta-2".into()),
        "the pane may still be running, so the row keeps naming it"
    );
}
