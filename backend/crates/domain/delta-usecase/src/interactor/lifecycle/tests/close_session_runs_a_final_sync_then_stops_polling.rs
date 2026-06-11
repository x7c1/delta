use crate::interactor::testing::*;

/// `close_session` captures the session's last transcript line before the
/// session leaves the open set, then the session is no longer polled.
///
/// Claude Code may flush the turn's final assistant line to the JSONL just
/// *after* its `Stop` hook fired. Once the session is closed the background tail
/// (`poll_transcript`) no longer polls it, so that straggler would be lost
/// without a final sync on close. This asserts both halves: the line written
/// right before close is ingested, and a later poll does not pick the session
/// up.
#[tokio::test]
async fn close_session_runs_a_final_sync_then_stops_polling() {
    let ix = interactor();
    // Spawn and bind an open session.
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
    let main = ix.store().main_thread_id(&id).await.unwrap();

    // A final assistant line is flushed to the JSONL after the turn's `Stop`,
    // before close — and has not been ingested yet.
    ix.transcript_fake()
        .push_to("/work/delta-1/t.jsonl", assistant_line("a-last", "last reply"));
    let before_close = ix.thread_view(main).await.unwrap();
    assert!(
        before_close.iter().all(|m| m.uuid.as_str() != "a-last"),
        "precondition: the last line is not ingested before close",
    );

    ix.close_session(&id).await.unwrap();

    // The final sync on close captured the straggler.
    let after_close = ix.thread_view(main).await.unwrap();
    assert!(
        after_close.iter().any(|m| m.uuid.as_str() == "a-last"),
        "close runs a final sync that captures the last line",
    );

    // The session is now closed, so the tail no longer polls it: even if its
    // shared JSONL grows again (e.g. an external resume), the tail ignores it.
    assert!(!ix.is_session_open(&id).await, "closed after close_session");
    ix.transcript_fake().push_to(
        "/work/delta-1/t.jsonl",
        assistant_line("a-after", "post-close growth"),
    );
    let (groups, _events) = ix.poll_transcript().await.unwrap();
    assert!(
        groups.is_empty(),
        "a closed session is no longer polled by the tail",
    );
    let final_view = ix.thread_view(main).await.unwrap();
    assert!(
        final_view.iter().all(|m| m.uuid.as_str() != "a-after"),
        "post-close growth is not ingested",
    );
}
