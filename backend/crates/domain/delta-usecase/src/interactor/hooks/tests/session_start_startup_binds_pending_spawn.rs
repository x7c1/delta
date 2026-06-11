use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// `SessionStart(source=startup)` for a pending spawn's pinned id binds and
/// registers it immediately — even with no first prompt — so a prompt-less plain
/// spawn registers without waiting for a `UserPromptSubmit`.
#[tokio::test]
async fn session_start_startup_binds_pending_spawn() {
    let ix = interactor();
    // Cold-start spawn (no first prompt).
    ix.new_session().await.unwrap();
    let session_id = ix.pending_session_ids().await.remove(0);
    assert!(ix.pane_for_session(&session_id).await.is_none());

    let events = ix
        .on_session_start(session_start(session_id.as_str(), "startup"))
        .await
        .unwrap();

    // It registered (the readiness signal doubles as registration) and bound the
    // spawn's pane.
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: session_id.clone(),
    }));
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        Some("delta-1:0.0".to_owned())
    );
    assert!(ix.store().session(&session_id).await.unwrap().is_some());
    // The spawn is no longer pending.
    assert!(ix.pending_session_ids().await.is_empty());
}
