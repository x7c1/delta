use delta_model::ThreadId;

use crate::interactor::testing::*;

#[tokio::test]
async fn enqueue_send_to_unknown_thread_is_thread_not_found() {
    use crate::error::Error;

    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();

    // A thread id that was never created (stale/wrong id from the browser).
    let err = ix
        .enqueue_send(to(ThreadId(999)), "hello", None)
        .await
        .expect_err("unknown thread must be rejected");
    assert!(matches!(err, Error::ThreadNotFound(999)));
}
