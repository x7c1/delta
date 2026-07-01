use std::sync::Arc;
use std::time::Duration;

use delta_model::SessionId;
use tokio::sync::Barrier;

use crate::interactor::testing::*;

/// Two sessions' transcript ingests run **at the same time**, while a third
/// session handles a hook to completion in between — the configuration the
/// old design could not express: one global sync lock serialized every
/// session's cursor→read→ingest sequence, so the shared barrier below (which
/// only opens once both reads are in flight *and* the third session's hook
/// has finished) would have deadlocked.
#[tokio::test]
async fn sessions_ingest_concurrently_while_a_third_handles_a_hook() {
    let ix = interactor();
    // Two open sessions with their own transcripts, bound so the tail polls
    // them.
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-1", &SessionId::from("sess-1"))
        .await;
    ix.on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "seed"))
        .await
        .unwrap();
    ix.bind_open_session("delta-2", &SessionId::from("sess-2"))
        .await;
    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-1", "reply one"));
    ix.transcript_fake()
        .push_to("/tmp/s2.jsonl", assistant_line("a-2", "reply two"));

    // Both reads must be parked on the same barrier simultaneously, and the
    // barrier needs a THIRD participant — the test itself, which only joins
    // after a different session's hook has been fully handled. So the poll
    // can only complete if (a) the two ingests overlap and (b) an unrelated
    // session's actor kept serving while they were parked.
    let gate = Arc::new(Barrier::new(3));
    ix.transcript_fake()
        .gate_reads("/tmp/s1.jsonl", gate.clone());
    ix.transcript_fake()
        .gate_reads("/tmp/s2.jsonl", gate.clone());

    let poll = ix.poll_transcript();
    let hook_then_release = async {
        // A third session registers via its hook while sess-1/sess-2 are
        // blocked mid-ingest: per-session mailboxes make this independent.
        let (events, _context) = ix
            .on_user_prompt_submit(submit_for("sess-3", "/tmp/s3.jsonl", "hello"))
            .await
            .unwrap();
        assert!(
            !events.is_empty(),
            "the third session's hook completed while the others ingested"
        );
        // Now release the two parked ingests.
        gate.wait().await;
    };

    let (poll_result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(poll, hook_then_release)
    })
    .await
    .expect("ingests must overlap; serialization would deadlock this barrier");

    let (groups, _events) = poll_result.unwrap();
    assert_eq!(groups.len(), 2, "each gated session ingested its line");
}
