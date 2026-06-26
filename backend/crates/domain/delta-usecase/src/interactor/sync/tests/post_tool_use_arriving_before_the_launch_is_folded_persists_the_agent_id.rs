use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

/// The race the `pending_post_tool_use_agent_ids` buffer was added to fix.
///
/// After the running-subagent indicator moved off the `PreToolUse` hook path
/// onto the parent-transcript ingest, a top-level background `Agent` launch
/// can land in this sequence:
///
/// 1. The assistant's `tool_use(Agent)` block has not yet been flushed to the
///    parent's JSONL when `PreToolUse(Agent)` fires, so the force-sync inside
///    the hook reads nothing and creates no in-memory entry, no launch row.
/// 2. `PostToolUse(Agent)` arrives next carrying `agentId` in `tool_result`.
///    The in-memory `upgrade_subagent_task_id` is a no-op (no entry yet) and
///    the matching DB UPDATE is skipped — without the buffer the value is
///    lost.
/// 3. A later sync finally folds the `tool_use(Agent)` line: the launch row is
///    INSERTed and the in-memory entry is created. The buffer must now flush
///    its stashed `agentId` onto both.
/// 4. A `<task-notification>` with only `<task-id>` (no `<tool-use-id>` —
///    Claude Code 2.1.193 does this for top-level background launches) then
///    finds the launch row by its persisted `task_id`, and the indicator
///    clears.
///
/// This test exercises exactly that ordering: PostToolUse FIRST, then the
/// transcript line is pushed and ingested by `on_pre_tool_use`'s force-sync.
#[tokio::test]
async fn post_tool_use_arriving_before_the_launch_is_folded_persists_the_agent_id() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // The parent's JSONL does NOT yet contain the `tool_use(Agent)` line.
    // `PostToolUse(Agent)` arrives first and reports `agentId` — the in-memory
    // upgrade has nothing to attach to, but the buffer must keep the value.
    let post_events = ix
        .on_post_tool_use(
            &session,
            "Agent",
            "toolu_bg",
            r#"{"agentId":"a-1"}"#,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();
    assert!(
        post_events.is_empty(),
        "PostToolUse with no running entry yet broadcasts nothing, got {post_events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "no in-memory running entry yet"
    );
    assert!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .is_empty(),
        "no launch row yet either"
    );

    // Now the parent flushes the assistant message: the `tool_use(Agent)`
    // line lands in the JSONL. `PreToolUse(Agent)` force-syncs the parent
    // transcript, which folds the line: `SubagentLaunched` INSERTs the
    // launch row, `SubagentIndicatorStarted` creates the in-memory entry
    // and drains the buffer, applying the `agentId` to both.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    let pre_events = ix
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
        pre_events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentStarted { background: true, tool_use_id, .. }
                if tool_use_id == "toolu_bg"
        )),
        "the force-sync inside PreToolUse lights the indicator, got {pre_events:?}"
    );

    // The in-memory entry now carries the buffered `agentId`.
    let state = ix.live_state_for(&session).await;
    let running = state
        .running_subagents
        .iter()
        .find(|s| s.tool_use_id == "toolu_bg")
        .expect("the background subagent is running");
    assert_eq!(
        running.task_id.as_deref(),
        Some("a-1"),
        "the buffered agentId was folded onto the freshly-created running entry"
    );

    // The launch row carries it too, so a `<task-notification>` missing
    // `<tool-use-id>` can still correlate.
    let launches = ix
        .store()
        .outstanding_subagent_launches(&session)
        .await
        .unwrap();
    let launch = launches
        .get("toolu_bg")
        .expect("a launch row exists for the background tool_use");
    assert_eq!(
        launch.task_id.as_deref(),
        Some("a-1"),
        "the buffered agentId was persisted on the launch row"
    );

    // The completion notification arrives with only `<task-id>` — the shape
    // Claude Code 2.1.193 produces for top-level background launches.
    // Correlation via the persisted `task_id` must still finish the entry.
    ix.transcript_fake()
        .push(task_notification_line_task_id_only("u-note", "a-1"));
    let note_events = ix
        .on_stop(StopHook {
            session_id: session.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();
    assert!(
        note_events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentFinished { session_id, tool_use_id }
                if *session_id == session && tool_use_id == "toolu_bg"
        )),
        "the task-id-only notification finishes the background subagent, got {note_events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the running subagent was cleared via the task-id fallback"
    );
}
