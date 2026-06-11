use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;

/// A queued command that matches no send — e.g. a background task notification
/// injected mid-turn — is a programmatic injection, not stray pane typing, so
/// it must inherit the active thread rather than reset attribution to `main`.
/// A child turn it lands in, and the assistant continuation after it, stay on
/// the child. This is the matched-only-switch rule for queued commands.
#[tokio::test]
async fn unmatched_queued_command_mid_branch_stays_on_branch() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Establish a branch turn: match its send so carry_thread is the child.
    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);
    ix.transcript_fake().push(user_line("u-b", "branch text"));
    ix.on_user_prompt_submit(submit("branch text"))
        .await
        .unwrap();

    // Mid-turn, a queued command with no matching send arrives, followed by the
    // assistant's continuation. Both ingested at Stop.
    ix.transcript_fake().push(queued_command_line(
        "u-note",
        "<task-notification>done</task-notification>",
    ));
    ix.transcript_fake()
        .push(assistant_line("a-after", "after the note"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // The uncorrelated queued command and the continuation stay on the branch.
    let child_uuids: Vec<String> = ix
        .thread_view(child)
        .await
        .unwrap()
        .iter()
        .map(|m| m.uuid.as_str().to_owned())
        .collect();
    assert!(
        child_uuids.contains(&"u-note".to_owned()),
        "an unmatched queued command inherits the branch, not main"
    );
    assert!(
        child_uuids.contains(&"a-after".to_owned()),
        "the continuation after it stays on the branch"
    );

    // Nothing leaked onto main.
    let main_uuids: Vec<String> = ix
        .thread_view(main)
        .await
        .unwrap()
        .iter()
        .map(|m| m.uuid.as_str().to_owned())
        .collect();
    assert!(!main_uuids.contains(&"u-note".to_owned()));
    assert!(!main_uuids.contains(&"a-after".to_owned()));
}
