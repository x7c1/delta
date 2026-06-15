use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;

/// End-to-end across sync windows: a background subagent launched in a branch
/// thread completes after the user has moved to `main`. The completion
/// notification — injected in a LATER sync window than the launch — must be
/// attributed back to the branch thread that launched it, not to `main`.
///
/// This pins the persistence path the pure fold cannot: the launch is recorded
/// in one `sync_transcript` call and the matching `<task-notification>` is
/// folded in a later one, so the correlation only survives if it round-trips
/// through the `subagent_launch` store between windows. Before the fix the
/// notification inherited the current (`main`) thread and landed there.
#[tokio::test]
async fn background_task_completion_lands_on_launching_thread() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Open a branch thread via a matched branch send, so the in-flight turn —
    // and `carry_thread` — is on the child when the background task launches.
    let parent = MessageUuid::from("uuid-parent");
    let (branch, _) = ix
        .enqueue_send(branch_off(main, &parent), "work the side topic", None)
        .await
        .unwrap();
    let child = branch.thread_id;
    assert_ne!(child, main);
    ix.transcript_fake()
        .push(user_line("u-b", "work the side topic"));
    ix.on_user_prompt_submit(submit("work the side topic"))
        .await
        .unwrap();

    // Window 1: the assistant launches a background subagent on the child
    // thread, then the turn stops. The launch correlation is now persisted.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .get("toolu_bg"),
        Some(&child),
        "the launch is recorded against the child thread"
    );

    // The user moves to `main` and works there — a plain trunk turn that
    // advances the carry thread to `main`.
    let (trunk, _) = ix
        .enqueue_send(to(main), "back to the trunk", None)
        .await
        .unwrap();
    assert_eq!(trunk.thread_id, main);
    ix.transcript_fake()
        .push(user_line("u-trunk", "back to the trunk"));
    ix.on_user_prompt_submit(submit("back to the trunk"))
        .await
        .unwrap();
    ix.transcript_fake()
        .push(assistant_line("a-trunk", "on the trunk now"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(
        ix.store().latest_user_thread(&session).await.unwrap(),
        Some(main),
        "the user is now working on main"
    );

    // Window 2: the background task completes. Its notification correlates
    // back to the launch and must land on the child thread, taking the
    // assistant's continuation with it.
    ix.transcript_fake()
        .push(task_notification_line("u-note", "toolu_bg"));
    ix.transcript_fake()
        .push(assistant_line("a-note", "the background agent finished"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    let child_view = ix.thread_view(child).await.unwrap();
    let child_uuids: Vec<&str> = child_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(
        child_uuids.contains(&"u-note"),
        "the completion notification lands on the launching (child) thread, got {child_uuids:?}"
    );
    assert!(
        child_uuids.contains(&"a-note"),
        "the assistant continuation follows onto the child thread"
    );

    // Neither leaked onto main.
    let main_view = ix.thread_view(main).await.unwrap();
    let main_uuids: Vec<&str> = main_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(!main_uuids.contains(&"u-note"));
    assert!(!main_uuids.contains(&"a-note"));

    // The launch correlation was consumed.
    assert!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .is_empty(),
        "the completion cleared the persisted launch"
    );
}
