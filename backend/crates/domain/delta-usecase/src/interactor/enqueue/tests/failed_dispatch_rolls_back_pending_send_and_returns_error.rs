use delta_model::SessionId;

use crate::interactor::testing::*;

#[tokio::test]
async fn failed_dispatch_rolls_back_pending_send_and_returns_error() {
    use crate::error::Error;

    let ix = interactor_with_failing_tmux();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    // Bind a live, ready pane so the send dispatches immediately on the normal
    // path (and fails there) rather than resuming the session — the resume gate
    // would hold the keystroke and no dispatch error would surface.
    ix.bind_open_session("delta-seed", &session).await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The dispatch fails, so the use case must surface the tmux error...
    let err = ix
        .enqueue_send(to(main), "never delivered", None)
        .await
        .expect_err("a failed dispatch must propagate the error");
    assert!(matches!(err, Error::Tmux(_)));

    // ...and the just-written row must not block the FIFO head: it was rolled
    // back to `cancelled`, so the head is clear for future correlation.
    let head = ix.store().head_pending_send(&session).await.unwrap();
    assert!(
        head.is_none(),
        "the cancelled row must not remain the FIFO head"
    );
}
