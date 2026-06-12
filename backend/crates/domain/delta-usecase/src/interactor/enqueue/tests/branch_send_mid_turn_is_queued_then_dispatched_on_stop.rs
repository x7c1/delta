use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::StopHook;

/// A branch send issued while a Delta-dispatched turn is in flight must be
/// held back (recorded `queued`, no keystrokes) rather than dispatched into
/// the busy pane, where Claude Code would queue it and its locator quote would
/// be lost. When the turn completes, it is dispatched as an ordinary prompt.
#[tokio::test]
async fn branch_send_mid_turn_is_queued_then_dispatched_on_stop() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A first send is dispatched immediately and marks the turn in flight.
    ix.enqueue_send(to(main), "first", None).await.unwrap();
    assert!(ix.store().is_turn_active(&session).await.unwrap());
    assert_eq!(ix.tmux_fake().sent.lock().unwrap().len(), 1);

    // A branch send arrives mid-turn: queued, not dispatched.
    let parent = MessageUuid::from("uuid-parent");
    let queued = ix
        .enqueue_send(branch_off(main, &parent), "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(queued.status, SendStatus::Queued);
    let child = queued.thread_id;
    assert_ne!(child, main, "the branch child thread is created up front");
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "no keystrokes dispatched for a queued send"
    );

    // The turn completes: the queued send is now dispatched as an ordinary
    // prompt (promoted, ready for correlation).
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    let (count, second) = {
        let sent = ix.tmux_fake().sent.lock().unwrap();
        (sent.len(), sent.get(1).map(|p| p.1.clone()))
    };
    assert_eq!(count, 2, "the queued send is dispatched at turn end");
    assert_eq!(second.as_deref(), Some("branch text"));
    assert!(
        ix.store().next_queued_send(&session).await.unwrap().is_none(),
        "the queued send was promoted and dispatched"
    );
}
