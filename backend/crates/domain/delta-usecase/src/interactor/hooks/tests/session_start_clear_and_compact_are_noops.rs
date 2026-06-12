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
        // The eagerly-created row exists from the spawn itself, but a
        // clear/compact must not activate it: it stays `spawning` with no
        // transcript path until a real launch signal binds the spawn.
        let session = ix
            .store()
            .session(&session_id)
            .await
            .unwrap()
            .expect("the spawn eagerly created the session row");
        assert_eq!(
            session.status,
            delta_model::SessionStatus::Spawning,
            "{source} does not activate the session"
        );
        assert_eq!(session.transcript_path, None);
        assert_eq!(
            ix.pending_session_ids().await.len(),
            1,
            "{source} leaves the pending spawn untouched"
        );
    }
}
