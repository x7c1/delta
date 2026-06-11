use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;

/// An interrupt ends the turn but fires no `Stop` hook, so the background tail
/// is where it is observed. A branch send deferred during that turn must be
/// dispatched once the tail ingests the interrupt marker — the user need not
/// send anything first.
#[tokio::test]
async fn deferred_branch_send_dispatches_after_interrupt() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Start a turn, then defer a branch send behind it.
    ix.enqueue_send(to(main), "first", None).await.unwrap();
    let parent = MessageUuid::from("uuid-parent");
    ix.enqueue_send(branch_off(main, &parent), "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(ix.tmux_fake().sent.lock().unwrap().len(), 1);

    // The user interrupts: Claude writes the marker, no Stop fires. The tail
    // ingests it and releases the deferred send.
    ix.transcript_fake().push(interrupt_line("uuid-interrupt"));
    ix.poll_transcript().await.unwrap();

    let (count, second) = {
        let sent = ix.tmux_fake().sent.lock().unwrap();
        (sent.len(), sent.get(1).map(|p| p.1.clone()))
    };
    assert_eq!(
        count, 2,
        "the deferred send is dispatched once the interrupt is tailed"
    );
    assert_eq!(second.as_deref(), Some("branch text"));
    assert!(ix
        .store()
        .next_deferred_send(&session)
        .await
        .unwrap()
        .is_none());
}
