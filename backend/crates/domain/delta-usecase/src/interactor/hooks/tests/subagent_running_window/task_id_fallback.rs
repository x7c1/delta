//! A background subagent whose `<task-notification>` lacks `<tool-use-id>`: finished by the launch's `agentId`, recorded at `PostToolUse`, or, when the body names no launch at all, as the only running one.

use delta_model::SessionId;

use super::*;
use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};

/// A background `Agent` whose `PostToolUse(Agent)` reported the launch's
/// `agentId` as `tool_response.agentId`: the upgrade is persisted on both the
/// runtime entry and the launch store row, so a later
/// `<task-notification>` whose `<tool-use-id>` element was stripped can still
/// match by `<task-id>`.
const POST_TOOL_USE_RESPONSE_WITH_AGENT_ID: &str = r#"{"agentId":"a31425032172620ed"}"#;

#[tokio::test]
async fn post_tool_use_upgrades_the_background_subagent_with_its_agent_id() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        BACKGROUND_AGENT_INPUT,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    // The background subagent's immediate `PostToolUse` carries the launch's
    // `agentId` in `tool_response`. The handler reads it, upgrades the running
    // entry, and persists the upgrade on the launch row (created when PreToolUse
    // force-synced the parent's tool_use line above).
    let events = ix
        .on_post_tool_use(
            &session,
            "Agent",
            "toolu_bg",
            POST_TOOL_USE_RESPONSE_WITH_AGENT_ID,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "an immediate background PostToolUse broadcasts nothing"
    );
    let state = ix.live_state_for(&session).await;
    let running = state
        .running_subagents
        .iter()
        .find(|s| s.tool_use_id == "toolu_bg")
        .expect("background subagent is still running");
    assert_eq!(
        running.task_id.as_deref(),
        Some("a31425032172620ed"),
        "the running entry was upgraded with the agentId"
    );
    let launch_task_id = ix
        .store()
        .outstanding_subagent_launches(&session)
        .await
        .unwrap()
        .get("toolu_bg")
        .and_then(|launch| launch.task_id.clone());
    assert_eq!(
        launch_task_id.as_deref(),
        Some("a31425032172620ed"),
        "the upgrade is persisted on the launch row too"
    );
}

#[tokio::test]
async fn a_task_notification_missing_tool_use_id_finishes_via_the_task_id_fallback() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // Launch a background subagent. The parent's JSONL carries the tool_use
    // line; PreToolUse force-syncs it, so the launch row exists; the immediate
    // PostToolUse then upgrades the launch row with the agentId.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        BACKGROUND_AGENT_INPUT,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    ix.on_post_tool_use(
        &session,
        "Agent",
        "toolu_bg",
        POST_TOOL_USE_RESPONSE_WITH_AGENT_ID,
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    // The completion notification arrives with ONLY `<task-id>` — Claude Code
    // stripped `<tool-use-id>` from the user-message body. The server must
    // still finish the background subagent via the fallback correlation.
    ix.transcript_fake()
        .push(task_notification_line_task_id_only(
            "u-note",
            "a31425032172620ed",
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
        "the task-id-only notification still emits SubagentFinished, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the running subagent was finished via the task-id fallback"
    );
    assert!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .is_empty(),
        "the launch correlation was cleared"
    );
}

#[tokio::test]
async fn a_task_notification_missing_both_ids_completes_the_only_running_subagent_and_warns() {
    // Capture warn-level tracing output so the test can assert the warn fires
    // when a `<task-notification>` body carries neither correlation element —
    // the keyless shape stays visible in the logs even when it is matched,
    // because it signals an upstream format change.
    let (logs, _guard) = capture_warnings();

    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    // Launch a background subagent and persist its launch row.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        BACKGROUND_AGENT_INPUT,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();

    // A notification body without either `<tool-use-id>` or `<task-id>` — a
    // Claude Code shape that names no launch — reports the ONLY outstanding
    // launch there is, so it completes it: the running indicator clears
    // instead of staying lit until the session is closed. The fold logs a
    // warn either way so the body shape stays visible.
    ix.transcript_fake()
        .push(task_notification_line_both_missing("u-note"));
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
            SessionEvent::SubagentFinished { tool_use_id, .. } if tool_use_id == "toolu_bg"
        )),
        "the only outstanding launch finishes on a no-key notification, got {events:?}"
    );
    assert!(
        running_tool_use_ids(&ix.live_state_for(&session).await).is_empty(),
        "the running entry is cleared — the notification can only be its completion"
    );

    let captured = logs.text();
    // The keyless body now has two possible warns — matched, and not matched
    // (zero or several outstanding) — so the assertion names the matched one
    // and the launch it resolved to; a warn that merely mentions
    // `<task-notification>` would pass for either.
    assert!(
        captured.contains("WARN")
            && captured.contains("matched it to the only outstanding background launch")
            && captured.contains("tool_use_id=toolu_bg"),
        "expected a WARN naming the launch the keyless notification matched, got: {captured}"
    );
}
