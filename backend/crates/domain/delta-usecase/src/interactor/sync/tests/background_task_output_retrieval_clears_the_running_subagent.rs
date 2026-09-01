use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

/// The parent retrieving a background subagent's result ITSELF — a blocking
/// `TaskOutput` call — is the other way a background run ends.
///
/// Claude Code enqueues a `<task-notification>` only for a completion the
/// parent did not ask for. When the parent blocks on `TaskOutput` no
/// notification ever fires, so the retrieval's own `tool_result` is the only
/// evidence the task is over. Without folding it the entry leaks forever: the
/// turn-end sweep deliberately keeps background entries, and the persisted
/// launch row re-seeds the spinner on every reload.
///
/// The correlation runs through `task_id` (a retrieval never names the
/// launching `tool_use_id`), learned from `PostToolUse(Agent)` exactly as the
/// task-id-fallback notification path learns it.
#[tokio::test]
async fn background_task_output_retrieval_clears_the_running_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // The assistant launches a background subagent. `PreToolUse(Agent)`
    // force-syncs the parent transcript, so the indicator lights and the
    // launch row is persisted on the same hook call.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    let started = ix
        .on_pre_tool_use(
            &session,
            "Agent",
            r#"{"subagent_type":"general-purpose","description":"Long crawl","run_in_background":true}"#,
            "toolu_bg",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();
    assert!(
        started.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentStarted {
                background: true,
                ..
            }
        )),
        "the launch started a background running entry, got {started:?}"
    );

    // The launch returns immediately, and its `PostToolUse(Agent)` reports the
    // `agentId` — the `task_id` the later retrieval names the task by.
    ix.on_post_tool_use(
        &session,
        "Agent",
        "toolu_bg",
        r#"{"agentId":"a-1"}"#,
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    // Window 1: the launching turn stops. A background subagent outlives its
    // turn, so it must still be running after the Stop.
    let after_launch = ix
        .on_stop(StopHook {
            session_id: session.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();
    assert!(
        !after_launch
            .iter()
            .any(|e| matches!(e, SessionEvent::SubagentFinished { .. })),
        "the launching turn ending must NOT finish the background subagent"
    );
    assert_eq!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .iter()
            .map(|s| s.tool_use_id.clone())
            .collect::<Vec<_>>(),
        vec!["toolu_bg".to_owned()],
        "the background subagent survives the launching turn"
    );

    // Window 2: the parent retrieves the result with a blocking `TaskOutput`.
    // NO `<task-notification>` is written — the retrieval's successful,
    // `completed` result is the whole completion signal.
    ix.transcript_fake()
        .push(task_output_tool_use_line("a-read", "toolu_read", "a-1"));
    ix.transcript_fake().push(task_output_result_line(
        "u-read",
        "toolu_read",
        "a-1",
        "completed",
        false,
    ));
    let events = ix
        .on_stop(StopHook {
            session_id: session.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();

    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentFinished { session_id, tool_use_id }
                if *session_id == session && tool_use_id == "toolu_bg"
        )),
        "folding the retrieval broadcasts SubagentFinished, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the retrieval cleared the running subagent"
    );
    assert!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .is_empty(),
        "the persisted launch row is cleared, so it cannot re-seed the spinner on reload"
    );
}
