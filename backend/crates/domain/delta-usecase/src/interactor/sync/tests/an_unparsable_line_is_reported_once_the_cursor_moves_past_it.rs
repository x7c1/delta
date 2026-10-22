use delta_model::SessionId;

use crate::interactor::testing::*;

const UNPARSABLE: &str = "skipping unparsable transcript line";

/// An unparsable transcript line is reported when the line cursor moves past
/// it, not when it is read. While the cursor write fails, every tick re-reads
/// the same range, and none of them reports the line; once the cursor write
/// succeeds the line is reported exactly once — at `warn`, naming the session,
/// the transcript, the line and the parser's error — and never again.
#[tokio::test]
async fn an_unparsable_line_is_reported_once_the_cursor_moves_past_it() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-1", &SessionId::from("sess-1"))
        .await;
    let (logs, _guard) = capture_warnings();
    let count = || logs.text().matches(UNPARSABLE).count();

    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-1", "before"));
    ix.transcript_fake()
        .push_unparsable_line_to("/tmp/s1.jsonl");
    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-2", "after"));

    ix.store()
        .inner
        .lock()
        .unwrap()
        .fail_set_transcript_lines_read = true;
    for _ in 0..3 {
        ix.poll_transcript(TICK_BOUND).await;
    }
    // Each failing tick read the line and got as far as the cursor write, so
    // the silence below is the deferred report, not a tick that never read.
    assert_eq!(
        logs.text()
            .matches("injected line cursor write failure")
            .count(),
        3,
        "every failing tick reached the cursor write: {}",
        logs.text()
    );
    assert_eq!(
        count(),
        0,
        "not reported while the cursor stays: {}",
        logs.text()
    );

    ix.store()
        .inner
        .lock()
        .unwrap()
        .fail_set_transcript_lines_read = false;
    let (groups, _events) = ix.poll_transcript(TICK_BOUND).await;
    assert_eq!(groups.len(), 1, "the lines around it are ingested");
    for _ in 0..3 {
        ix.poll_transcript(TICK_BOUND).await;
    }
    assert_eq!(count(), 1, "reported exactly once: {}", logs.text());
    let logged = logs.text();
    let line = logged.lines().find(|l| l.contains(UNPARSABLE)).unwrap();
    assert!(
        line.contains("WARN")
            && line.contains("sess-1")
            && line.contains("/tmp/s1.jsonl line 2")
            && line.contains("expected"),
        "the warning names the session, the file, the line and the cause: {line}"
    );
}
