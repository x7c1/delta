use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// A `spawning` row with no launch behind it at all — what a server restart
/// mid-launch leaves — is cleaned up by a close.
///
/// Open/closed and every launch record are runtime state, rebuilt empty on
/// restart, while the eager row lives in the store: after a restart the row is
/// still `spawning`, nothing is pending for the watchdog to reap, and no hook
/// will ever arrive to bind it. That card would sit amber forever, which is why
/// the cleanup is keyed on the row's status rather than on having a launch
/// record or a pane to tear down. No pane token travels on the event: this
/// runtime never minted one.
#[tokio::test]
async fn close_session_cancels_a_spawning_row_left_by_a_restart() {
    let ix = interactor();
    let session_id = SessionId::from("sess-stranded");

    // The eager row a pre-restart spawn wrote, with none of the runtime state
    // that spawn had: no launching entry, no pending spawn, nothing bound.
    ix.store()
        .insert_spawning_session(spawning_session(&session_id, "/work"))
        .await
        .unwrap();
    assert!(
        !ix.is_session_open(&session_id).await,
        "a restart leaves the row without a live pane"
    );

    let events = ix.close_session(&session_id).await.unwrap();

    assert_eq!(
        events,
        vec![SessionEvent::SpawnFailed {
            session_id: session_id.clone(),
            pane_token: None,
            reason: Some("closed while starting".to_owned()),
            cancelled: true,
            unsent: Vec::new(),
        }],
    );
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the stranded `spawning` row is deleted, so the amber card leaves the list"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "this runtime holds no pane token, so tmux is left alone"
    );
}
