use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

/// A background subagent launch that is DENIED by the permission / auto-mode
/// classifier still writes its `tool_use` block to the parent JSONL, so the
/// running indicator lights and the launch row is persisted — but the launch
/// never actually happened, so no completion `<task-notification>` will ever
/// arrive, and the turn-end sweep keeps background entries. Without a dedicated
/// clear the indicator would be stuck forever.
///
/// The denial surfaces as an `is_error: true` `tool_result` for the launching
/// `tool_use_id`. Folding it must emit `Effect::SubagentCompleted`, which
/// clears the persisted launch row, drops the running entry, and broadcasts
/// `SubagentFinished` so the navigator badge / conversation indicator clears —
/// exactly as a real completion notification would.
#[tokio::test]
async fn denied_background_launch_clears_the_running_subagent() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // The assistant attempts to launch a background subagent. The parent's
    // JSONL carries the `tool_use(Agent)` block; `PreToolUse(Agent)` force-syncs
    // the parent transcript so the indicator lights and the launch row is
    // persisted on the same hook call.
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
        "the launch attempt started a background running entry, got {started:?}"
    );

    // Window 1: the launching turn stops. A background subagent outlives its
    // turn, so it is still running after the Stop — this is exactly why a denied
    // launch would otherwise stay stuck.
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
        "the background launch survives the launching turn"
    );

    // Window 2: the denial lands as an `is_error: true` `tool_result` for the
    // launching `tool_use_id`. Folding it emits `SubagentCompleted`, which
    // finishes the running entry and broadcasts `SubagentFinished`.
    ix.transcript_fake()
        .push(errored_tool_result_line("u-denied", "toolu_bg"));
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
        "folding the denial's errored tool_result broadcasts SubagentFinished, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the denial cleared the stuck running subagent"
    );
}
