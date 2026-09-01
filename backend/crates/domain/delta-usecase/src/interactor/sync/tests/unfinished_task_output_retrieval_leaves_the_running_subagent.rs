use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

/// The other half of the `TaskOutput` fold: a retrieval that does NOT report a
/// finished task must leave the running entry alone.
///
/// Two shapes reach here and neither says the subagent is over:
///
/// - a non-blocking poll of a task still working, whose body carries
///   `<status>running</status>`; and
/// - a retrieval that itself failed (`is_error: true`), which reports nothing
///   about the task's state at all.
///
/// Clearing on either would blank the spinner while the subagent is still
/// producing output — the opposite failure to the leak the fold fixes.
#[tokio::test]
async fn unfinished_task_output_retrieval_leaves_the_running_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        r#"{"subagent_type":"general-purpose","description":"Long crawl","run_in_background":true}"#,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    ix.on_post_tool_use(
        &session,
        "Agent",
        "toolu_bg",
        r#"{"agentId":"a-1"}"#,
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // A non-blocking poll: the task is still working.
    ix.transcript_fake()
        .push(task_output_tool_use_line("a-poll", "toolu_poll", "a-1"));
    ix.transcript_fake().push(task_output_result_line(
        "u-poll",
        "toolu_poll",
        "a-1",
        "running",
        false,
    ));
    // A retrieval that failed outright: its `<status>` is meaningless.
    ix.transcript_fake()
        .push(task_output_tool_use_line("a-read", "toolu_read", "a-1"));
    ix.transcript_fake().push(task_output_result_line(
        "u-read",
        "toolu_read",
        "a-1",
        "completed",
        true,
    ));
    let events = ix
        .on_stop(StopHook {
            session_id: session.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::SubagentFinished { .. })),
        "neither a running poll nor an errored retrieval finishes the subagent, got {events:?}"
    );
    assert_eq!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .iter()
            .map(|s| s.tool_use_id.clone())
            .collect::<Vec<_>>(),
        vec!["toolu_bg".to_owned()],
        "the background subagent is still running"
    );
    assert!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .contains_key("toolu_bg"),
        "the launch row survives, so a later terminal retrieval can still correlate"
    );
}
