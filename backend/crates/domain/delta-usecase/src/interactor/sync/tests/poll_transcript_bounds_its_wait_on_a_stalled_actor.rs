//! A tick waits only so long for any one actor.
//!
//! The fan-out used to await every reply indefinitely, so one actor stuck
//! mid-read held the whole tick — and with it every other session's result.
//! The caller now hands the fan-out a bound: an actor that misses it is warned
//! about and skipped for that tick, while the sessions that answered are
//! returned on time. Nothing is lost — the skipped actor keeps working on its
//! own mailbox, and its rows land as soon as it can read them.

use std::sync::Arc;
use std::time::{Duration, Instant};

use delta_model::SessionId;
use tokio::sync::Barrier;

use crate::interactor::testing::*;

/// Short enough that the test is quick, long enough that a loaded machine
/// still lets the free session's actor answer inside it.
const SHORT_BOUND: Duration = Duration::from_millis(200);

#[tokio::test]
async fn poll_transcript_bounds_its_wait_on_a_stalled_actor() {
    let ix = interactor();
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

    // Session one's reads park until the test joins the barrier; session two's
    // are free.
    let stalled = Arc::new(Barrier::new(2));
    ix.transcript_fake()
        .gate_reads("/tmp/s1.jsonl", stalled.clone());
    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-1", "reply one"));
    ix.transcript_fake()
        .push_to("/tmp/s2.jsonl", assistant_line("a-2", "reply two"));

    // The tick ends on the bound rather than on session one, which is still
    // parked with nothing about to release it.
    let started = Instant::now();
    let (groups, _events) =
        tokio::time::timeout(Duration::from_secs(5), ix.poll_transcript(SHORT_BOUND))
            .await
            .expect("the bound ends the fan-out; session one never replies")
            .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the tick returned on its bound, not on the stalled actor"
    );
    assert_eq!(groups.len(), 1, "only the session that answered in time");
    assert_eq!(
        groups[0][0].session_id,
        SessionId::from("sess-2"),
        "the free session's lines came back on time"
    );

    // The skipped actor was never abandoned: release its read and drive one
    // more tick (whose own read parks on the same barrier, released the same
    // way). Its mailbox is ordered, so by the time that tick is handled the
    // ingest the timed-out tick started has been persisted.
    stalled.wait().await;
    let (polled, _released) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(ix.poll_transcript(TICK_BOUND), stalled.wait())
    })
    .await
    .expect("the released actor answers the next tick");
    polled.unwrap();

    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();
    let uuids: Vec<_> = ix
        .store()
        .thread_messages(main)
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.uuid.as_str().to_owned())
        .collect();
    assert!(
        uuids.contains(&"a-1".to_owned()),
        "the stalled session's line was ingested, not dropped: {uuids:?}"
    );
}
