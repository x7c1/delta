use delta_model::{MessageUuid, ThreadId};

use crate::interactor::testing::*;

/// A branch send targeting a thread that does not exist is rejected with
/// `ThreadNotFound`. A branch send always names the parent thread it hangs off,
/// so with no such thread (no session has been registered) there is nothing to
/// branch from — and it must not silently spawn a fresh session.
#[tokio::test]
async fn branch_send_to_unknown_thread_is_thread_not_found() {
    use crate::error::Error;

    let ix = interactor();
    let parent = MessageUuid::from("uuid-parent");
    let err = ix
        .enqueue_send(branch_off(ThreadId(1), &parent), "branch text", None)
        .await
        .expect_err("a branch send needs an existing parent thread");
    assert!(matches!(err, Error::ThreadNotFound(_)));
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a rejected branch send must not spawn a session"
    );
}
