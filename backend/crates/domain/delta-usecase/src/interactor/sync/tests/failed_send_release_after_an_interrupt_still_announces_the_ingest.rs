//! A failed queued-send release must not swallow the tick's announcement.
//!
//! Ingesting an interrupt marker ends the turn, so the tail releases whatever
//! was queued behind it — which types into the pane, and can fail. That failure
//! used to propagate out of the tick: the fan-out returned `Err`, and the lines
//! this session had *already persisted* were never announced. No later tick
//! re-offers them (the cursor has moved past them), so the browser would never
//! refetch what was ingested — the same silence the per-session announcement
//! exists to prevent. The release is now logged and skipped instead, mirroring
//! how the resume and echo-deadline ticks already treat a dispatch failure.

use std::time::Duration;

use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;
use crate::ports::{AsyncEventSink, SessionEvent};

#[tokio::test]
async fn failed_send_release_after_an_interrupt_still_announces_the_ingest() {
    let (sink, mut events) = AsyncEventSink::channel();
    let ix = interactor_with_failing_tmux().with_event_sink(sink);
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A turn started by a prompt typed straight into the pane — no dispatch, so
    // the failing tmux is not in the way yet — with a branch send queued behind
    // it. That row is what the tail will try to release.
    ix.transcript_fake()
        .push(user_line("u-ext", "typed in the pane"));
    ix.on_user_prompt_submit(submit("typed in the pane"))
        .await
        .unwrap();
    let parent = MessageUuid::from("uuid-parent");
    ix.enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();
    assert!(
        ix.store()
            .next_queued_send(&session)
            .await
            .unwrap()
            .is_some(),
        "the branch send waits behind the in-flight turn"
    );

    // The user interrupts: the marker line is ingested, the turn ends, and the
    // release of the queued send fails on the dead pane.
    ix.transcript_fake().push(interrupt_line("u-int"));
    let (groups, _events) = ix
        .poll_transcript(TICK_BOUND)
        .await
        .expect("the failed release is logged, not propagated out of the tick");
    assert_eq!(groups.len(), 1, "the marker line was ingested");
    assert!(
        ix.store()
            .next_queued_send(&session)
            .await
            .unwrap()
            .is_none(),
        "the release was attempted — its failure cancelled the row it promoted"
    );

    // And the browser still hears about the lines that landed.
    let announced = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.expect("the event seam stays open") {
                SessionEvent::TranscriptUpdated { session_id, .. } => return session_id,
                _ => continue,
            }
        }
    })
    .await
    .expect("the ingest is announced despite the failed release");
    assert_eq!(announced, session);
}
