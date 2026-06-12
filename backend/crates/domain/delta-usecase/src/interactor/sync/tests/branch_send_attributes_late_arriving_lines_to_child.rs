use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::context::{frame_branch_entry_context, frame_locator_context};
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// Reproduces the thread-attribution timing bug: the `UserPromptSubmit` hook
/// fires before the user line is written to the JSONL, so nothing is attributed
/// in that sync. Both the user line and the assistant reply arrive together in a
/// later sync (as happens at `Stop`) and must still land on the branch thread.
#[tokio::test]
async fn branch_send_attributes_late_arriving_lines_to_child() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Queue a branch send. The user line is NOT in the transcript yet.
    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(
            branch_off(main, &parent),
            "branch text",
            Some("quoted line"),
        )
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);

    // The hook fires before the user line is flushed to the JSONL. The locator
    // quote frame (plus the branch-entry note) is still returned (text-based),
    // but nothing is attributed yet.
    let (events, additional) = ix
        .on_user_prompt_submit(submit("branch text"))
        .await
        .unwrap();
    let expected =
        frame_branch_entry_context(&frame_locator_context("quoted line").unwrap(), child);
    assert_eq!(additional, Some(expected));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::TurnStarted { .. })),
        "no turn started while the user line is absent"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "a queued send matched, so this is not external input"
    );
    // Still pending: nothing was matched yet.
    let head = ix.store().head_dispatched_send(&session).await.unwrap();
    assert_eq!(head.map(|p| p.id), Some(pending.id));

    // Later (at Stop) BOTH the user line and the assistant reply arrive in one
    // sync. Attribution must key off the pending send, not the hook timing.
    ix.transcript_fake().push(user_line("u-b", "branch text"));
    ix.transcript_fake()
        .push(assistant_line("a-b", "branch reply"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // Both messages land on the child thread.
    let child_view = ix.thread_view(child).await.unwrap();
    let child_uuids: Vec<&str> = child_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(child_uuids.contains(&"u-b"), "user message lands on child");
    assert!(
        child_uuids.contains(&"a-b"),
        "assistant message lands on child"
    );

    // The user message carries the branch semantic parent.
    let user_msg = child_view
        .iter()
        .find(|m| m.uuid.as_str() == "u-b")
        .unwrap();
    assert_eq!(user_msg.semantic_parent_uuid, Some(parent));

    // The pending send is now matched (to the user line uuid).
    let send = ix
        .store()
        .inner
        .lock()
        .unwrap()
        .sends
        .iter()
        .find(|s| s.id == pending.id)
        .cloned()
        .unwrap();
    assert_eq!(send.status, SendStatus::Matched);
    assert_eq!(send.matched_uuid, Some(MessageUuid::from("u-b")));

    // Neither leaked onto main.
    let main_view = ix.thread_view(main).await.unwrap();
    let main_uuids: Vec<&str> = main_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(!main_uuids.contains(&"u-b"));
    assert!(!main_uuids.contains(&"a-b"));
}
