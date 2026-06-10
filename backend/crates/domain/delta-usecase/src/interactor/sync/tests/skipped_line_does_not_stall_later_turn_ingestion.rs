use delta_model::SessionId;

use crate::interactor::testing::*;

/// Regression test for the line-vs-message offset stall.
///
/// A no-uuid line (Claude Code's `file-history-snapshot`) trails turn 1. With
/// the old message-count offset, the cursor (a message count) lagged behind the
/// file line count by one for every skipped line, so the next sync re-read
/// already-ingested lines, `seq` drifted, and the latest turn stopped being
/// ingested. With the line-based cursor, the skipped line still advances the
/// cursor, so turn 2 is ingested cleanly on the second sync.
#[tokio::test]
async fn skipped_line_does_not_stall_later_turn_ingestion() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // --- Sync 1: turn 1 (user + assistant) followed by a no-uuid line. ---
    ix.enqueue_send(to(main), "turn one", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-1", "turn one")); // line 0
    ix.transcript_fake()
        .push(assistant_line("a-1", "reply one")); // line 1
    ix.transcript_fake().push_skipped_line(); // line 2: file-history-snapshot
    ix.on_user_prompt_submit(submit("turn one")).await.unwrap();
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // Turn 1 is ingested and the cursor advanced past the trailing no-uuid line
    // (3 lines), not merely past the 2 messages.
    let after1 = ix.thread_view(main).await.unwrap();
    let uuids1: Vec<&str> = after1.iter().map(|m| m.uuid.as_str()).collect();
    assert!(uuids1.contains(&"u-1"));
    assert!(uuids1.contains(&"a-1"));
    assert_eq!(
        ix.store().transcript_lines_read(&session).await.unwrap(),
        3,
        "the cursor counts the no-uuid line, not just the messages"
    );

    // --- Sync 2: turn 2 appended. Previously this stalled. ---
    ix.enqueue_send(to(main), "turn two", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-2", "turn two")); // line 3
    ix.transcript_fake()
        .push(assistant_line("a-2", "reply two")); // line 4
    ix.on_user_prompt_submit(submit("turn two")).await.unwrap();
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    let view = ix.thread_view(main).await.unwrap();
    let uuids: Vec<&str> = view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(uuids.contains(&"u-2"), "turn 2 user is ingested");
    assert!(uuids.contains(&"a-2"), "turn 2 assistant is ingested");

    // seq follows the true file line order (line indices), monotonic and gapless
    // across the skipped line, with no duplicates.
    let by_uuid = |u: &str| view.iter().find(|m| m.uuid.as_str() == u).unwrap().seq;
    assert_eq!(by_uuid("u-1"), 0);
    assert_eq!(by_uuid("a-1"), 1);
    assert_eq!(by_uuid("u-2"), 3);
    assert_eq!(by_uuid("a-2"), 4);
    assert_eq!(view.len(), 4, "no duplicates from re-reading lines");
    // thread_view orders by seq; assert it is strictly increasing (monotonic,
    // no duplicate line indices).
    let seqs: Vec<i64> = view.iter().map(|m| m.seq).collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "seq is strictly increasing in line order: {seqs:?}"
    );
}
