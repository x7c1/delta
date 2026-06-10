use delta_model::{MessageUuid, SessionId};

use crate::interactor::testing::*;

/// A tool call mid-turn on a branch must not reset attribution to `main`. Claude
/// writes the `tool_result` as a `role: user` line; treating it as a new human
/// turn used to drop the result and the assistant's continuation onto `main`, so
/// the branch lost the turn's tail (its last message). Regression test.
#[tokio::test]
async fn tool_result_mid_branch_turn_stays_on_the_branch() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Start a branch turn and match its user line onto the child thread.
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

    // The turn calls a tool: assistant tool_use, the tool_result (a `role: user`
    // line), then the assistant's final text — all ingested together at Stop.
    ix.transcript_fake()
        .push(tool_use_line("a-call", "t1", "Bash"));
    ix.transcript_fake().push(tool_result_line("u-res", "t1"));
    ix.transcript_fake()
        .push(assistant_line("a-final", "after the tool"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // The whole tail stays on the branch.
    let child_uuids: Vec<String> = ix
        .thread_view(child)
        .await
        .unwrap()
        .iter()
        .map(|m| m.uuid.as_str().to_owned())
        .collect();
    assert!(child_uuids.contains(&"a-call".to_owned()));
    assert!(
        child_uuids.contains(&"u-res".to_owned()),
        "tool_result stays on the branch turn, not main"
    );
    assert!(
        child_uuids.contains(&"a-final".to_owned()),
        "the assistant continuation after the tool stays on the branch"
    );

    // Nothing leaked onto main.
    let main_uuids: Vec<String> = ix
        .thread_view(main)
        .await
        .unwrap()
        .iter()
        .map(|m| m.uuid.as_str().to_owned())
        .collect();
    assert!(!main_uuids.contains(&"u-res".to_owned()));
    assert!(!main_uuids.contains(&"a-final".to_owned()));
}
