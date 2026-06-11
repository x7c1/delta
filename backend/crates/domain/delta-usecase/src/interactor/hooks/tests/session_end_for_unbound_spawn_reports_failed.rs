use crate::interactor::testing::*;
use crate::ports::{SessionEndHook, SessionEvent};

/// A `SessionEnd` for a still-unbound pending spawn is a failed launch: the
/// spawn is removed, its pane is killed, and a `SpawnFailed` is emitted.
#[tokio::test]
async fn session_end_for_unbound_spawn_reports_failed() {
    let ix = interactor();

    // Cold-start spawn that never received its first UserPromptSubmit, so it is
    // still pending (unbound).
    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);

    let events = ix
        .on_session_end(SessionEndHook {
            session_id: id.clone(),
            reason: Some("exit".into()),
        })
        .await
        .unwrap();

    // SpawnFailed carries the spawn's id and pane token.
    assert_eq!(
        events,
        vec![SessionEvent::SpawnFailed {
            session_id: id.clone(),
            pane_token: "delta-1".to_owned(),
        }],
    );
    // The pane was killed and the spawn is gone, so it can never mis-bind later.
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-1".to_owned()],
    );
    assert!(ix.pending_session_ids().await.is_empty());
}
