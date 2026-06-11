use crate::interactor::testing::*;

/// `SessionStart(source=clear)` and `source=compact` fire mid-session on an
/// already-live session. They are not launches, so they must not bind a pending
/// spawn or register anything — they are safe no-ops.
#[tokio::test]
async fn session_start_clear_and_compact_are_noops() {
    for source in ["clear", "compact"] {
        let ix = interactor();
        // A pending spawn is waiting; a clear/compact must NOT bind it (those
        // sources never name a launch).
        ix.new_session().await.unwrap();
        let session_id = ix.pending_session_ids().await.remove(0);

        let events = ix
            .on_session_start(session_start(session_id.as_str(), source))
            .await
            .unwrap();

        assert!(events.is_empty(), "{source} emits no events");
        assert!(
            ix.store().session(&session_id).await.unwrap().is_none(),
            "{source} does not register the session"
        );
        assert_eq!(
            ix.pending_session_ids().await.len(),
            1,
            "{source} leaves the pending spawn untouched"
        );
    }
}
