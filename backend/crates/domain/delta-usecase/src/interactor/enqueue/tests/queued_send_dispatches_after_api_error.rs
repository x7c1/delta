use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;

/// A turn that ends on an API error (a usage/session limit, a rate limit, or any
/// other API failure) fires no `Stop` hook and writes no interrupt marker —
/// Claude only records a synthetic `isApiErrorMessage` assistant line. Without a
/// transcript-driven fallback the turn machine would stay in flight forever, so
/// a send composed during that turn would defer to `queued` and never dispatch
/// (the production symptom: a permanently "pending" chip after a usage limit).
///
/// Ingesting the api-error line must instead return the turn machine to `Idle`
/// and release the queued send — exactly as the interrupt-marker fallback does —
/// so the send is dispatched without the user having to send anything first.
#[tokio::test]
async fn queued_send_dispatches_after_api_error() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Start a turn (the dispatch echoes back and its user line is ingested, the
    // realistic ordering), then defer a branch send behind it.
    ix.enqueue_send(to(main), "first", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-first", "first"));
    ix.on_user_prompt_submit(submit("first")).await.unwrap();
    let parent = MessageUuid::from("uuid-parent");
    ix.enqueue_send(branch_off(main, &parent), "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().len(),
        1,
        "the second send is held queued while the first turn is in flight"
    );

    // The turn hits a usage/session limit: Claude writes the synthetic
    // `isApiErrorMessage` line, no Stop hook fires. The tail ingests it,
    // returns the turn machine to idle, and releases the queued send.
    ix.transcript_fake().push(api_error_line("uuid-api-error"));
    ix.poll_transcript().await.unwrap();

    let (count, second) = {
        let sent = ix.tmux_fake().sent.lock().unwrap();
        (sent.len(), sent.get(1).map(|p| p.1.clone()))
    };
    assert_eq!(
        count, 2,
        "the queued send is dispatched once the api-error line is tailed"
    );
    assert_eq!(second.as_deref(), Some("branch text"));
    assert!(
        ix.store()
            .next_queued_send(&session)
            .await
            .unwrap()
            .is_none(),
        "no send remains queued: it left `queued` and dispatched"
    );
}
