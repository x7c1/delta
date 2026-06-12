use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::interactor::session_actor::runtime::RESUME_DISPATCH_SETTLE;

/// `SessionStart(source=resume)` only *marks* the resume ready: it must NOT type
/// the held first prompt from the handler. That hook blocks `claude` until it
/// returns, so a keystroke sent here would land while `claude` is still inside
/// the hook and not accepting input, and be lost. The handler leaves the entry
/// in the resuming map with its `ready_at` stamped; the held prompt is dispatched
/// only later, by `dispatch_ready_resumes` on the background tick. We confirm
/// `ready_at` was set by showing the dispatch fires once the settle elapses.
#[tokio::test]
async fn session_start_resume_marks_ready_without_dispatching() {
    let ix = interactor();
    let session_id = SessionId::from("sess-mark-ready");

    // A resuming session with a held first prompt and a live pane.
    ix.push_resuming_at(
        "delta-2",
        &session_id,
        Some("held prompt".to_owned()),
        Instant::now(),
    )
    .await;

    // The readiness hook fires.
    ix.on_session_start(session_start(session_id.as_str(), "resume"))
        .await
        .unwrap();

    // No keystroke was dispatched from the handler...
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "the resume readiness hook does not dispatch from the handler"
    );
    // ...and the session is still resuming (ready, pending dispatch on the tick).
    assert_eq!(
        ix.resuming_session_ids().await,
        vec![session_id.clone()],
        "the marked-ready resume stays in the resuming map awaiting the dispatch tick"
    );

    // Proof the hook stamped `ready_at`: once `now` advances past the settle, the
    // dispatch tick types the held prompt (a never-stamped resume would not).
    ix.dispatch_ready_resumes(Instant::now() + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert!(
        sent.iter()
            .any(|(p, t)| p == "delta-2:0.0" && t == "held prompt"),
        "the held prompt dispatched on the settle tick, confirming ready_at was set, got {sent:?}"
    );
    assert!(ix.resuming_session_ids().await.is_empty());
}
