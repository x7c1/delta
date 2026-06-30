use crate::interactor::testing::*;

/// `SessionStart(source=clear)` fires mid-session on an already-live session
/// when the user wipes the context. It is not a launch, so it must not bind
/// a pending spawn or register anything — a clear is a deliberate context
/// wipe, and resurrecting prior sends would invert intent, so it stays a
/// safe no-op (no events, no re-dispatch, no spawn binding).
#[tokio::test]
async fn session_start_clear_is_noop() {
    let ix = interactor();
    // A pending spawn is waiting; a clear must NOT bind it (clear never
    // names a launch).
    ix.new_session().await.unwrap();
    let session_id = ix.pending_session_ids().await.remove(0);

    let events = ix
        .on_session_start(session_start(session_id.as_str(), "clear"))
        .await
        .unwrap();

    assert!(events.is_empty(), "clear emits no events");
    // The eagerly-created row exists from the spawn itself, but a clear
    // must not activate it: it stays `spawning` with no transcript path
    // until a real launch signal binds the spawn.
    let session = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the spawn eagerly created the session row");
    assert_eq!(
        session.status,
        delta_model::SessionStatus::Spawning,
        "clear does not activate the session"
    );
    assert_eq!(session.transcript_path, None);
    assert_eq!(
        ix.pending_session_ids().await.len(),
        1,
        "clear leaves the pending spawn untouched"
    );
    // No tmux keystrokes are sent — a clear must not resurrect prior sends.
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "clear must not re-type any send"
    );
}
