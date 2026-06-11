use std::time::{Duration, Instant};

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::open_sessions::RESUME_DISPATCH_SETTLE;

/// The dispatch tick honours `RESUME_DISPATCH_SETTLE`: a resume marked ready is
/// NOT dispatched before the settle elapses, and IS dispatched (and removed from
/// the resuming map) once `now` has advanced past it. This is the second stage
/// of the readiness gate — the keystroke is typed on the tick, after the
/// (blocking) `SessionStart(resume)` hook returned, not from inside it.
#[tokio::test]
async fn dispatch_ready_resumes_holds_until_settle_then_dispatches() {
    let ix = interactor();
    let ready_at = Instant::now();
    let session_id = SessionId::from("sess-settle");

    // A resuming session with a held first prompt and a live pane, recorded
    // before its ready stamp.
    ix.push_resuming_at(
        "delta-5",
        &session_id,
        Some("held prompt".to_owned()),
        ready_at - Duration::from_secs(1),
    )
    .await;
    // SessionStart(resume) marked it ready at `ready_at`.
    assert!(ix.mark_resume_ready_at(&session_id, ready_at).await);

    // Before the settle elapses, the dispatch tick holds the keystroke.
    let before_settle = ready_at + RESUME_DISPATCH_SETTLE - Duration::from_millis(1);
    ix.dispatch_ready_resumes(before_settle).await.unwrap();
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "the held prompt is not dispatched before the settle elapses"
    );
    assert_eq!(
        ix.resuming_session_ids().await,
        vec![session_id.clone()],
        "the resume stays in the map until it is dispatched"
    );

    // Once `now` reaches the settle, the held prompt is typed and the resume
    // leaves the map.
    let at_settle = ready_at + RESUME_DISPATCH_SETTLE;
    ix.dispatch_ready_resumes(at_settle).await.unwrap();
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert!(
        sent.iter()
            .any(|(p, t)| p == "delta-5:0.0" && t == "held prompt"),
        "the held first prompt dispatched into the resumed pane once settled, got {sent:?}"
    );
    assert!(
        ix.resuming_session_ids().await.is_empty(),
        "the dispatched resume is removed from the resuming map"
    );
}
