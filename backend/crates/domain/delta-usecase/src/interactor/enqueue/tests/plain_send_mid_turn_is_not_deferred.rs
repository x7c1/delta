use delta_model::{PendingSendStatus, SessionId};

use crate::interactor::testing::*;

/// Deferral is scoped to sends that carry thread context (a branch entry or a
/// locator quote). A plain main-line send issued mid-turn needs no quote, so
/// Claude Code's own mid-turn queueing is harmless for it: it dispatches
/// immediately rather than being held back.
#[tokio::test]
async fn plain_send_mid_turn_is_not_deferred() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Start a turn.
    ix.enqueue_send(to(main), "first", None).await.unwrap();
    assert!(ix.store().is_turn_active(&session).await.unwrap());

    // A plain main-line send mid-turn (no branch, no quote) dispatches now.
    let second = ix.enqueue_send(to(main), "second", None).await.unwrap();
    assert_eq!(second.status, PendingSendStatus::Pending);
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        2,
        "the plain mid-turn send is dispatched immediately"
    );
    assert!(ix
        .store()
        .next_deferred_send(&session)
        .await
        .unwrap()
        .is_none());
}
