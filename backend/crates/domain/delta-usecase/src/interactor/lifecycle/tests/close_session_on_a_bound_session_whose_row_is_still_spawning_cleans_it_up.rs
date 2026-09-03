use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// The defensive third starting shape: a session whose pane is *bound* but
/// whose row never left `spawning`.
///
/// A bind whose row activation failed is unreachable from the hook path now
/// that a spawn stays pending until its registration succeeds, but rows left
/// that way by an older build can still exist and nothing else repairs them —
/// they show as an amber `Starting` card with a live pane behind it. Closing
/// one tears the pane down as usual and then cleans the row up, so the card
/// leaves the list instead of staying amber forever.
#[tokio::test]
async fn close_session_on_a_bound_session_whose_row_is_still_spawning_cleans_it_up() {
    let ix = interactor();
    let session_id = SessionId::from("sess-wedged");

    // The eager row a real spawn writes, plus a bound pane — the state a bind
    // that failed to activate its row leaves behind.
    ix.store()
        .insert_spawning_session(spawning_session(&session_id, "/work"))
        .await
        .unwrap();
    ix.bind_open_session("delta-7", &session_id).await;
    assert!(ix.is_session_open(&session_id).await, "open before close");

    let events = ix.close_session(&session_id).await.unwrap();

    // The pane is torn down by the normal close path…
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-7".to_owned()],
    );
    assert!(!ix.is_session_open(&session_id).await, "closed");
    // …and the never-activated row is cleaned up and reported, so the amber
    // card disappears rather than outliving its pane.
    assert_eq!(
        events,
        vec![SessionEvent::SpawnFailed {
            session_id: session_id.clone(),
            pane_token: Some("delta-7".to_owned()),
            reason: Some("closed while starting".to_owned()),
            cancelled: true,
            unsent: Vec::new(),
        }],
    );
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the contentless `spawning` row is deleted"
    );
}
