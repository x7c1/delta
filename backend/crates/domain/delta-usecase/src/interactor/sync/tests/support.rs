use delta_model::{MessageUuid, SessionId, ThreadId};

use crate::interactor::testing::*;
use crate::Interactor;

/// Register a known-but-closed session that has a prior *branch* turn pending
/// (a child thread plus a queued branch send matching `prior branch prompt`),
/// returning the interactor and the `(session, main, child)` ids. The branch
/// send and child thread are written via the store directly, NOT through
/// `enqueue_send`, so the closed session is not resumed yet (going through
/// `enqueue_send` would open it early and trip the double-open guard on the
/// explicit `open_session` under test).
pub(super) async fn closed_session_with_pending_branch() -> (
    Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace>,
    SessionId,
    ThreadId,
    ThreadId,
) {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let parent = MessageUuid::from("uuid-parent");
    let child = ix
        .store()
        .create_thread(&id, "prior branch prompt", Some(main))
        .await
        .unwrap()
        .id;
    ix.store()
        .enqueue_send(&id, child, Some(&parent), "prior branch prompt", None)
        .await
        .unwrap();
    (ix, id, main, child)
}

/// The thread a given ingested message landed on, by uuid.
pub(super) fn ingested_thread(
    ix: &Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace>,
    uuid: &str,
) -> Option<ThreadId> {
    ix.store()
        .inner
        .lock()
        .unwrap()
        .messages
        .iter()
        .find(|m| m.uuid.as_str() == uuid)
        .map(|m| m.thread_id)
}
