//! The incident regression: one session's stalled transcript read must not
//! silence every other session's announcement.
//!
//! The tail used to announce from the *loop*: `poll_transcript` posted a tick
//! to every actor, awaited them all, and only then did the server turn each
//! returned group into a `TranscriptUpdated`. A single actor slow to answer
//! therefore held back every other session's event — their rows were already
//! persisted, but a browser waiting on that event to refetch showed nothing.
//! Now each actor announces its own batch from inside the mailbox that
//! serialized the ingest, so the announcement leaves at the moment the rows
//! land.

use std::sync::Arc;
use std::time::Duration;

use delta_model::SessionId;
use tokio::sync::Barrier;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

#[tokio::test]
async fn each_session_announces_its_own_ingest_while_another_stalls() {
    let (ix, mut sink) = interactor_with_event_sink();
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

    // Session one's read parks until the test joins the barrier — the stalled
    // actor. Session two's read is free. Gated only now, so the registration
    // hooks above ran unimpeded.
    let stalled = Arc::new(Barrier::new(2));
    ix.transcript_fake()
        .gate_reads("/tmp/s1.jsonl", stalled.clone());

    // Both sessions flush a late assistant line.
    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-1", "reply one"));
    ix.transcript_fake()
        .push_to("/tmp/s2.jsonl", assistant_line("a-2", "reply two"));

    let poll = ix.poll_transcript(TICK_BOUND);
    let observe = async {
        // Session two's announcement must arrive while session one is still
        // parked mid-read — the ordering the loop-side announcement could not
        // produce.
        let announced = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match sink.recv().await.expect("the event seam stays open") {
                    SessionEvent::TranscriptUpdated {
                        session_id,
                        thread_ids,
                    } => return (session_id, thread_ids),
                    _ => continue,
                }
            }
        })
        .await
        .expect("session two announces without waiting on session one's read");
        assert_eq!(
            announced.0,
            SessionId::from("sess-2"),
            "the free session announced its own ingest first"
        );
        assert_eq!(
            announced.1.len(),
            1,
            "the announcement names the one thread the line landed on"
        );
        // Only now let session one's read complete.
        stalled.wait().await;
    };

    let (polled, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(poll, observe)
    })
    .await
    .expect("the stalled read is released, so the poll finishes");

    let (groups, _events) = polled.unwrap();
    assert_eq!(
        groups.len(),
        2,
        "both sessions ingested once the stalled read was released"
    );
}
