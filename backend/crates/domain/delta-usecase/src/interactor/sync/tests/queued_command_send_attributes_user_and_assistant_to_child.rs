use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;

/// A branch send issued while a turn is in flight is queued by Claude and
/// recorded only as a `queued_command` attachment — never a normal user line,
/// and with no `UserPromptSubmit` hook of its own. Sync must still correlate
/// that attachment to its queued send so the prompt AND the reply that follows
/// land on the child thread, not `main`. Regression for the bug where a
/// mid-turn branch send left its sub-thread empty and the reply on `main`.
#[tokio::test]
async fn queued_command_send_attributes_user_and_assistant_to_child() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Branch off an existing message and queue the first branch send.
    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);

    // The prompt was composed mid-turn, so it appears only as a queued_command
    // attachment (no UserPromptSubmit), and the reply follows in the same turn.
    // Both are ingested together at Stop.
    ix.transcript_fake()
        .push(queued_command_line("u-b", "branch text"));
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
    assert!(
        child_uuids.contains(&"u-b"),
        "the queued prompt lands on the child"
    );
    assert!(
        child_uuids.contains(&"a-b"),
        "the reply carries forward to the child"
    );

    // The matched queued prompt also carries the branch semantic parent.
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
