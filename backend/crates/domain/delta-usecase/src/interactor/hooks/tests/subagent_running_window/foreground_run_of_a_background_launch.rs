//! A subagent recorded as background that Claude Code ran in the foreground: its `PostToolUse` result decides whether the entry finishes.

use delta_model::SessionId;

use super::*;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// What `PostToolUse(Agent)` reports as `tool_response` for a subagent Claude
/// Code ran in the FOREGROUND even though the tool_use carried no
/// `run_in_background` key (it decides per subagent type).
const POST_TOOL_USE_RESPONSE_COMPLETED: &str = r#"{"status":"completed","agentId":"a31425032172620ed","totalDurationMs":5321,"totalTokens":1234,"usage":{}}"#;

/// What `PostToolUse(Agent)` reports as `tool_response` for a background launch.
const POST_TOOL_USE_RESPONSE_ASYNC_LAUNCHED: &str =
    r#"{"status":"async_launched","agentId":"a31425032172620ed"}"#;

#[tokio::test]
async fn a_completed_post_tool_use_finishes_a_subagent_recorded_as_background() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // The modern launch shape is recorded as background, with a launch row.
    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch",
        "toolu_fg",
        "Agent",
        "web-fetch",
        "Fetch the page",
    ));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        AGENT_INPUT,
        "toolu_fg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    assert!(is_background(
        &ix.live_state_for(&session).await,
        "toolu_fg"
    ));
    assert!(ix
        .store()
        .outstanding_subagent_launches(&session)
        .await
        .unwrap()
        .contains_key("toolu_fg"));

    // Claude Code ran it in the foreground: the hook fires when the subagent
    // finished, with `status: completed`. No `<task-notification>` follows.
    let events = ix
        .on_post_tool_use(
            &session,
            "Agent",
            "toolu_fg",
            POST_TOOL_USE_RESPONSE_COMPLETED,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert_eq!(
        events,
        vec![SessionEvent::SubagentFinished {
            session_id: session.clone(),
            tool_use_id: "toolu_fg".to_owned(),
        }],
        "a completed run finishes the background-recorded entry"
    );
    assert!(
        running_tool_use_ids(&ix.live_state_for(&session).await).is_empty(),
        "the running entry is gone"
    );
    assert!(
        !ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .contains_key("toolu_fg"),
        "the persisted launch row is removed"
    );
}

#[tokio::test]
async fn an_async_launched_post_tool_use_leaves_the_background_subagent_running() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.transcript_fake().push(agent_tool_use_line(
        "a-launch",
        "toolu_bg",
        "Agent",
        "general-purpose",
        "Run ls and count entries",
    ));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        AGENT_INPUT,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    let events = ix
        .on_post_tool_use(
            &session,
            "Agent",
            "toolu_bg",
            POST_TOOL_USE_RESPONSE_ASYNC_LAUNCHED,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert!(events.is_empty(), "a background launch broadcasts nothing");
    let state = ix.live_state_for(&session).await;
    assert_eq!(running_tool_use_ids(&state), vec!["toolu_bg".to_owned()]);
    let launch = ix
        .store()
        .outstanding_subagent_launches(&session)
        .await
        .unwrap()
        .remove("toolu_bg")
        .expect("the launch row stays for the completion notification");
    assert_eq!(launch.task_id.as_deref(), Some("a31425032172620ed"));
}
