use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::TranscriptRefusal;

const REFUSED: &str = "the transcript is refused";
const RECOVERED: &str = "the transcript is read again after failing";

/// A transcript the reader refuses is refused again on every tick, so the
/// background tail reports it once — at `warn`, naming the session and the
/// reason — and then stays quiet while the same refusal repeats. A different
/// refusal is news, and so is a refusal after the session recovered (the
/// recovery itself is logged at `info`). The tick never fails for it.
#[tokio::test]
async fn a_refused_transcript_is_reported_once_per_refusal() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-1", &SessionId::from("sess-1"))
        .await;
    let (logs, _guard) = capture_logs(tracing::Level::INFO);
    let count = |needle: &str| logs.text().matches(needle).count();

    ix.transcript_fake()
        .refuse("/tmp/s1.jsonl", TranscriptRefusal::NotRegularFile);
    for _ in 0..3 {
        let (groups, events) = ix.poll_transcript(TICK_BOUND).await;
        assert!(groups.is_empty() && events.is_empty());
    }
    assert_eq!(count(REFUSED), 1, "reported once: {}", logs.text());
    let logged = logs.text();
    let line = logged.lines().find(|l| l.contains(REFUSED)).unwrap();
    assert!(
        line.contains("WARN") && line.contains("sess-1") && line.contains("not a regular file"),
        "the warning names the session and the reason: {line}"
    );

    // Another refusal (here, another size) is reported again, once.
    let too_large = |size| TranscriptRefusal::TooLarge { size, cap: 10 };
    ix.transcript_fake().refuse("/tmp/s1.jsonl", too_large(11));
    ix.poll_transcript(TICK_BOUND).await;
    ix.poll_transcript(TICK_BOUND).await;
    ix.transcript_fake().refuse("/tmp/s1.jsonl", too_large(12));
    ix.poll_transcript(TICK_BOUND).await;
    ix.poll_transcript(TICK_BOUND).await;
    assert_eq!(count(REFUSED), 3, "one report per refusal: {}", logs.text());
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
    assert!(line.contains("INFO") && line.contains("sess-1"), "{line}");

    // The same refusal as before the recovery is news again.
    ix.transcript_fake().refuse("/tmp/s1.jsonl", too_large(12));
    ix.poll_transcript(TICK_BOUND).await;
    ix.poll_transcript(TICK_BOUND).await;
    assert_eq!(
        count(REFUSED),
        4,
        "reported again after recovering: {}",
        logs.text()
    );
}
