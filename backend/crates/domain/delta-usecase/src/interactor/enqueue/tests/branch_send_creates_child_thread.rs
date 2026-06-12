use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;

#[tokio::test]
async fn branch_send_creates_child_thread() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let main = ix
        .store()
        .main_thread_id(&SessionId::from("sess-1"))
        .await
        .unwrap();

    let parent = MessageUuid::from("uuid-parent");
    let (send, _) = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();

    assert_ne!(send.thread_id, main, "branch send targets a new thread");
    assert_eq!(send.semantic_parent_uuid, Some(parent.clone()));
    let child = ix.store().thread(send.thread_id).await.unwrap().unwrap();
    assert_eq!(child.parent_thread_id, Some(main));
    assert_eq!(child.root_message_uuid, Some(parent));
}
