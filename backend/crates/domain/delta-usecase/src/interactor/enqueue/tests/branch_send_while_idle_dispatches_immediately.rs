use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::testing::*;

/// Deferral only applies while a turn is in flight. A branch send to an idle
/// session dispatches immediately on the normal path, so its `UserPromptSubmit`
/// hook fires and the locator quote is injected as usual — no need to defer.
#[tokio::test]
async fn branch_send_while_idle_dispatches_immediately() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The session is idle (the registration turn completed, none in flight).
    assert!(!ix.store().is_turn_active(&session).await.unwrap());

    let parent = MessageUuid::from("uuid-parent");
    let send = ix
        .enqueue_send(branch_off(main, &parent), "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    assert_ne!(send.thread_id, main);
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "the branch send is dispatched immediately when idle"
    );
    assert!(ix.store().is_turn_active(&session).await.unwrap());
}
