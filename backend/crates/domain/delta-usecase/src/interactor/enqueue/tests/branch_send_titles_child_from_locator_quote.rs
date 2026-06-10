use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;

/// A branch send creates the child thread with a provisional title derived from
/// the locator quote, instead of the placeholder "untitled".
#[tokio::test]
async fn branch_send_titles_child_from_locator_quote() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let main = ix
        .store()
        .main_thread_id(&SessionId::from("sess-1"))
        .await
        .unwrap();

    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(
            branch_off(main, &parent),
            "branch text",
            Some("  the quoted source line  "),
        )
        .await
        .unwrap();
    let child = ix.store().thread(pending.thread_id).await.unwrap().unwrap();
    assert_eq!(child.title, "the quoted source line");

    // With no quote, the title falls back to "untitled".
    let pending2 = ix
        .enqueue_send(branch_off(main, &parent), "branch text 2", None)
        .await
        .unwrap();
    let child2 = ix
        .store()
        .thread(pending2.thread_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(child2.title, "untitled");
}
