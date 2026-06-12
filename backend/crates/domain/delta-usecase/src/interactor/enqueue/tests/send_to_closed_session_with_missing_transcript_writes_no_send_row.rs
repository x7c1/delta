use delta_model::SessionId;

use crate::interactor::testing::*;

/// A Send to a closed session whose transcript is gone fails before any send
/// row is written: `ensure_open` resumes via `open_session`, which now refuses
/// with `ResumeUnavailable`, so `enqueue_into_open` never runs. This is the
/// fix for the "stuck waiting indicator" — without an optimistic send row,
/// the UI has nothing to leave hanging.
#[tokio::test]
async fn send_to_closed_session_with_missing_transcript_writes_no_send_row() {
    use crate::error::Error;

    let ix = interactor();
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    ix.transcript_fake().mark_missing("/elsewhere/t.jsonl");
    let main = ix.store().main_thread_id(&id).await.unwrap();

    let err = ix
        .enqueue_send(to(main), "after resume", None)
        .await
        .expect_err("a send to a resume-impossible session must fail");
    assert!(
        matches!(err, Error::ResumeUnavailable(ref s) if s == "sess-R"),
        "the failure propagates as ResumeUnavailable, got: {err:?}"
    );

    // The key assertion: no optimistic send row sits at the FIFO head waiting
    // for a hook that will never fire.
    assert!(
        ix.store().head_dispatched_send(&id).await.unwrap().is_none(),
        "no send row was enqueued"
    );
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "no keystrokes were dispatched"
    );
    assert!(
        ix.pane_for_session(&id).await.is_none(),
        "the session stays closed"
    );
}
