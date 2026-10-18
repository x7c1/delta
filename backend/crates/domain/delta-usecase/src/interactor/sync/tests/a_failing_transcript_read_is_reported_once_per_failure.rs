use std::io::ErrorKind;

use delta_model::SessionId;

use crate::interactor::testing::*;

const FAILED: &str = "the transcript could not be read";
const RECOVERED: &str = "the transcript is read again after failing";

/// A transcript read that fails in a way that may pass (here, an I/O error)
/// tends to fail the same way on every tick, so the background tail reports
/// it once — at `warn`, naming the session and the cause — and then stays
/// quiet while the same failure repeats. A different failure is news, and so
/// is the same failure after the session recovered (the recovery itself is
/// logged at `info`). The tick never fails for it.
#[tokio::test]
async fn a_failing_transcript_read_is_reported_once_per_failure() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-1", &SessionId::from("sess-1"))
        .await;
    let (logs, _guard) = capture_logs(tracing::Level::INFO);
    let count = |needle: &str| logs.text().matches(needle).count();

    ix.transcript_fake()
        .fail_reads("/tmp/s1.jsonl", ErrorKind::PermissionDenied);
    for _ in 0..3 {
        let (groups, events) = ix.poll_transcript(TICK_BOUND).await;
        assert!(groups.is_empty() && events.is_empty());
    }
    assert_eq!(count(FAILED), 1, "reported once: {}", logs.text());
    let logged = logs.text();
    let line = logged.lines().find(|l| l.contains(FAILED)).unwrap();
    assert!(
        line.contains("WARN")
            && line.contains("sess-1")
            && line.contains("/tmp/s1.jsonl")
            && line.contains("permission denied"),
        "the warning names the session, the file and the cause: {line}"
    );

    // Another failure is reported again, once.
    ix.transcript_fake()
        .fail_reads("/tmp/s1.jsonl", ErrorKind::TimedOut);
    ix.poll_transcript(TICK_BOUND).await;
    ix.poll_transcript(TICK_BOUND).await;
    assert_eq!(count(FAILED), 2, "one report per failure: {}", logs.text());
    assert_eq!(count(RECOVERED), 0);

    // The session recovers: the recovery is logged once at `info`, and the
    // lines written meanwhile are ingested.
    ix.transcript_fake().stop_failing("/tmp/s1.jsonl");
    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-1", "reply"));
    let (groups, _events) = ix.poll_transcript(TICK_BOUND).await;
    assert_eq!(groups.len(), 1, "the recovered session is ingested again");
    ix.poll_transcript(TICK_BOUND).await;
    assert_eq!(count(RECOVERED), 1, "recovery logged once: {}", logs.text());
    let logged = logs.text();
    let line = logged.lines().find(|l| l.contains(RECOVERED)).unwrap();
    assert!(
        line.contains("INFO") && line.contains("sess-1") && line.contains("timed out"),
        "the recovery names the session and what it recovered from: {line}"
    );

    // The same failure as before the recovery is news again.
    ix.transcript_fake()
        .fail_reads("/tmp/s1.jsonl", ErrorKind::TimedOut);
    ix.poll_transcript(TICK_BOUND).await;
    ix.poll_transcript(TICK_BOUND).await;
    assert_eq!(
        count(FAILED),
        3,
        "reported again after recovering: {}",
        logs.text()
    );
}
