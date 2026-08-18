use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// The `agentId` Claude Code mints for the forked skill; it is also the
/// `<task-id>` its completion notification carries.
const AGENT_ID: &str = "a7046b32df40e1b3e";
/// The synthetic `tool_use_id` Delta tracks the forked skill under — a forked
/// skill writes no `tool_use` block, so it has no real `toolu_...` id.
const FORKED_TOOL_USE_ID: &str = "forked-skill:a7046b32df40e1b3e";

/// A session started with a slash command whose skill runs in the BACKGROUND
/// (`/review-pr`, which Claude Code records as `/example:review-pr`) used
/// to show no running indicator at all for the minutes the skill worked.
///
/// Two things happen in one transcript group, and only the second was missing:
///
/// 1. The local command is folded as a degenerate, already-finished turn
///    (`SendMatched` + `LocalCommandTurnEnded`) — a local command fires no echo
///    and no `Stop`, so without it the dispatched send wedges the queue. This
///    is correct, and it means the `turn_started`-driven half of the indicator
///    is legitimately dark.
/// 2. The forked skill itself is launched by the CLI harness, not by the model,
///    so the parent transcript carries NO `tool_use` block — only a
///    `<forked-skill-launch>` element on the command's system line. Nothing
///    registered it as running, so the subagent-driven half was dark too.
///
/// Ingesting the group must therefore light a BACKGROUND running-subagent entry
/// that SURVIVES the turn end emitted by the very same group.
#[tokio::test]
async fn forked_skill_launch_lights_a_background_subagent_that_survives_the_turn_end() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The user runs `/review-pr`: Delta dispatches it and the turn machine is
    // `AwaitingEcho` for that send.
    ix.enqueue_send(to(main), "/review-pr", None).await.unwrap();

    // Claude writes the local-command group, then the system line carrying the
    // forked-skill launch (no promptId of its own — it is not a member of the
    // group's promptId set).
    ix.transcript_fake()
        .push(local_command_caveat_line("caveat", "pcmd"));
    ix.transcript_fake().push(local_command_name_line(
        "cmdname",
        "pcmd",
        "/example:review-pr",
    ));
    ix.transcript_fake().push(forked_skill_launch_line(
        "forked",
        AGENT_ID,
        "example:review-pr",
    ));
    let (_groups, events) = ix.poll_transcript().await.unwrap();

    // The launch is broadcast as a background subagent on the launching thread,
    // labelled with the skill.
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentStarted {
                session_id,
                thread_id,
                tool_use_id,
                subagent_type,
                description,
                background: true,
            } if *session_id == session
                && *thread_id == main
                && tool_use_id == FORKED_TOOL_USE_ID
                && subagent_type.as_deref() == Some("example:review-pr")
                && description.as_deref() == Some("/example:review-pr")
        )),
        "the forked skill broadcasts SubagentStarted, got {events:?}"
    );
    // The same group ended the degenerate turn (that is the mechanism that used
    // to leave the row completely inert)...
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnInterrupted { session_id, .. } if *session_id == session
        )),
        "the local command still ends its degenerate turn, got {events:?}"
    );
    // ...and the background entry SURVIVES it: the running indicator stays lit
    // while the forked skill works.
    let running = ix.live_state_for(&session).await.running_subagents;
    assert_eq!(
        running
            .iter()
            .map(|s| s.tool_use_id.clone())
            .collect::<Vec<_>>(),
        vec![FORKED_TOOL_USE_ID.to_owned()],
        "the forked skill is still running after the local command's turn end"
    );
    assert!(running[0].background, "a forked skill is always background");

    // The launch row carries the `agentId` as its `task_id` in one step — the
    // payload knew it at launch — so a completion landing in a LATER sync
    // window still correlates by `<task-id>`.
    let launches = ix
        .store()
        .outstanding_subagent_launches(&session)
        .await
        .unwrap();
    let launch = launches
        .get(FORKED_TOOL_USE_ID)
        .expect("a launch row exists for the forked skill");
    assert_eq!(launch.thread_id, main);
    assert_eq!(launch.task_id.as_deref(), Some(AGENT_ID));
}

/// The forked skill's completion arrives as a `<task-notification>` carrying
/// only `<task-id>` (there never was a tool_use, so there is no
/// `<tool-use-id>`). It must finish the entry the launch lit: broadcast
/// `SubagentFinished`, drop the running entry, and clear the persisted launch
/// row.
#[tokio::test]
async fn forked_skill_completion_notification_clears_the_indicator() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;

    ix.transcript_fake().push(forked_skill_launch_line(
        "forked",
        AGENT_ID,
        "example:review-pr",
    ));
    ix.poll_transcript().await.unwrap();

    // Minutes later the forked agent finishes and the harness injects the
    // notification.
    ix.transcript_fake()
        .push(task_notification_line_task_id_only("u-note", AGENT_ID));
    let (_groups, events) = ix.poll_transcript().await.unwrap();

    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SubagentFinished { session_id, tool_use_id }
                if *session_id == session && tool_use_id == FORKED_TOOL_USE_ID
        )),
        "the task-id-only notification finishes the forked skill, got {events:?}"
    );
    assert!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .is_empty(),
        "the completion cleared the running indicator"
    );
    assert!(
        ix.store()
            .outstanding_subagent_launches(&session)
            .await
            .unwrap()
            .is_empty(),
        "the completion cleared the persisted launch row"
    );
}

/// Re-ingesting the launch line (a cursor rewind, e.g. after a restart that
/// replays the transcript) must be a no-op: `start_subagent` de-duplicates by
/// `tool_use_id`, so no second entry appears and no second `SubagentStarted`
/// is broadcast — otherwise the navigator would show a phantom running
/// subagent that nothing can ever finish.
#[tokio::test]
async fn re_ingesting_the_forked_skill_launch_does_not_duplicate_the_indicator() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;

    ix.transcript_fake().push(forked_skill_launch_line(
        "forked",
        AGENT_ID,
        "example:review-pr",
    ));
    ix.poll_transcript().await.unwrap();

    // Rewind the read cursor so the same line is folded a second time.
    ix.store()
        .set_transcript_lines_read(&session, 0)
        .await
        .unwrap();
    let (_groups, events) = ix.poll_transcript().await.unwrap();

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::SubagentStarted { .. })),
        "a re-folded launch broadcasts nothing new, got {events:?}"
    );
    assert_eq!(
        ix.live_state_for(&session)
            .await
            .running_subagents
            .iter()
            .map(|s| s.tool_use_id.clone())
            .collect::<Vec<_>>(),
        vec![FORKED_TOOL_USE_ID.to_owned()],
        "the forked skill is tracked exactly once"
    );
}
