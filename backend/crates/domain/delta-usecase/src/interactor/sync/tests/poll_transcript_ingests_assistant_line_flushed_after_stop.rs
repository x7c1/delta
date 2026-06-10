use delta_model::SessionId;

use crate::interactor::testing::*;

/// Reproduces the core "responses don't appear" bug: Claude Code flushes the
/// final assistant line to the JSONL *after* the `Stop` hook fires, so the
/// hook's sync misses it. Only a later `poll_transcript` (the continuous tail)
/// ingests it and returns it.
#[tokio::test]
async fn poll_transcript_ingests_assistant_line_flushed_after_stop() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Queue a send and run the user-turn hooks. At `Stop` only the user line is
    // present — the assistant reply has not been flushed yet.
    ix.enqueue_send(to(main), "hello world", None)
        .await
        .unwrap();
    ix.transcript_fake().push(user_line("u-1", "hello world"));
    ix.on_user_prompt_submit(submit("hello world"))
        .await
        .unwrap();
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // The user line is ingested, but the assistant reply is absent: the gap the
    // hook-only ingestion leaves.
    let after_stop = ix.thread_view(main).await.unwrap();
    let uuids_after_stop: Vec<&str> = after_stop.iter().map(|m| m.uuid.as_str()).collect();
    assert!(
        uuids_after_stop.contains(&"u-1"),
        "user line ingested at hooks"
    );
    assert!(
        !uuids_after_stop.contains(&"a-1"),
        "assistant reply is not ingested by the hooks (the bug)"
    );

    // Claude Code now flushes the assistant line. A poll (no hook) catches it.
    // The single session yields one group carrying just the new line.
    ix.transcript_fake().push(assistant_line("a-1", "hi there"));
    let (polled, _events) = ix.poll_transcript().await.unwrap();
    assert_eq!(
        polled.len(),
        1,
        "one group for the single registered session"
    );
    let polled_uuids: Vec<&str> = polled[0].iter().map(|m| m.uuid.as_str()).collect();
    assert_eq!(polled_uuids, vec!["a-1"], "poll returns only the new line");
    assert_eq!(
        polled[0][0].thread_id, main,
        "the assistant reply is attributed to the turn's thread"
    );

    // It is now persisted on the thread.
    let final_view = ix.thread_view(main).await.unwrap();
    let final_uuids: Vec<&str> = final_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(
        final_uuids.contains(&"a-1"),
        "poll ingested the assistant reply"
    );

    // A second poll with no new lines returns nothing (cursor advanced).
    let (again, _events) = ix.poll_transcript().await.unwrap();
    assert!(again.is_empty(), "no new lines, nothing returned");
}
