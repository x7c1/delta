use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::Error;

/// `open_sends_for` is the read side of the browser's pending-send strip: it
/// returns the session's non-terminal sends (`queued` or `dispatched`) oldest
/// first, and reports an unknown session as a clean `SessionNotFound` so a
/// reaped spawn is distinguishable from "nothing pending".
#[tokio::test]
async fn open_sends_for_lists_open_sends_and_rejects_unknown_session() {
    let ix = interactor();
    ix.seed_session().await;
    let session_id = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session_id).await.unwrap();

    // One dispatched, one queued, one already matched (terminal).
    let dispatched = ix
        .store()
        .enqueue_send(&session_id, main, None, "dispatched", None)
        .await
        .unwrap();
    let queued = ix
        .store()
        .enqueue_queued_send(&session_id, main, None, "queued", None)
        .await
        .unwrap();
    let matched = ix
        .store()
        .enqueue_send(&session_id, main, None, "matched", None)
        .await
        .unwrap();
    ix.store()
        .mark_send_matched(matched.id, &MessageUuid::from("u-1"))
        .await
        .unwrap();

    let open = ix.open_sends_for(&session_id).await.unwrap();
    let ids: Vec<i64> = open.iter().map(|s| s.id).collect();
    assert_eq!(
        ids,
        vec![dispatched.id, queued.id],
        "non-terminal sends only, oldest first"
    );
    assert_eq!(open[0].status, SendStatus::Dispatched);
    assert_eq!(open[1].status, SendStatus::Queued);

    // An unknown id is an error, not an empty list.
    let err = ix
        .open_sends_for(&SessionId::from("ghost"))
        .await
        .expect_err("unknown session must not yield a silently empty list");
    assert!(matches!(err, Error::SessionNotFound(id) if id == "ghost"));
}
