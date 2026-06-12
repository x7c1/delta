use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;

#[tokio::test]
async fn branch_send_attributes_user_and_assistant_to_child() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Branch off some existing message and queue the first branch send.
    let parent = MessageUuid::from("uuid-parent");
    let (pending, _) = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);

    // The matching user line is present at submit time, so it is matched to the
    // pending send and attributed to the child during this sync.
    ix.transcript_fake().push(user_line("u-b", "branch text"));
    ix.on_user_prompt_submit(submit("branch text"))
        .await
        .unwrap();

    // The assistant response is ingested at Stop and must carry forward to the
    // child thread (the thread of the latest user message).
    ix.transcript_fake()
        .push(assistant_line("a-b", "branch reply"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // Both land on the child, not main.
    let child_view = ix.thread_view(child).await.unwrap();
    let child_uuids: Vec<&str> = child_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(child_uuids.contains(&"u-b"), "user message lands on child");
    assert!(
        child_uuids.contains(&"a-b"),
        "assistant message lands on child"
    );

    // The matched user message also carries the branch semantic parent.
    let user_msg = child_view
        .iter()
        .find(|m| m.uuid.as_str() == "u-b")
        .unwrap();
    assert_eq!(user_msg.semantic_parent_uuid, Some(parent));

    // And neither leaked onto main.
    let main_view = ix.thread_view(main).await.unwrap();
    let main_uuids: Vec<&str> = main_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(!main_uuids.contains(&"u-b"));
    assert!(!main_uuids.contains(&"a-b"));
}
