use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

/// A background subagent (`run_in_background: true`) survives the launching
/// turn and is finished only when its completion `<task-notification>` is
/// folded during transcript sync.
///
/// `PreToolUse(Agent)` starts the running entry (marked background); its
/// immediate `PostToolUse` and the launching turn ending both leave it running.
/// Then, in a later sync window, the `<task-notification>` is folded: the
/// `Effect::SubagentCompleted` it emits must clear the running entry and
/// broadcast `SubagentFinished`, so the navigator badge / conversation
/// indicator disappears.
#[tokio::test]
async fn background_task_completion_clears_the_running_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // The assistant launches a background subagent. The parent's JSONL carries
    // the `tool_use(Agent)` block; `PreToolUse(Agent)` force-syncs the parent
    // transcript so the indicator lights and the launch row is persisted on
    // the same hook call.
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

    // Window 2: the background task completes. Folding the `<task-notification>`
    // emits `SubagentCompleted`, which finishes the running entry and broadcasts
    // `SubagentFinished`.
    ix.transcript_fake()
        .push(task_notification_line("u-note", "toolu_bg"));
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
        "folding the completion notification broadcasts SubagentFinished, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the completion cleared the running subagent"
    );
}
