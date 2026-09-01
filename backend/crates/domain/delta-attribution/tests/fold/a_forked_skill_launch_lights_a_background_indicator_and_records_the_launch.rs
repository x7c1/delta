use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn a_forked_skill_launch_lights_a_background_indicator_and_records_the_launch() {
    // The reported bug: a session started with a slash command that forks its
    // skill into a background agent (`/review-pr`, recorded as
    // `/example:review-pr`) showed NO running indicator for the minutes
    // the skill worked. The forked skill is launched by the CLI harness, not
    // by the model, so the parent transcript carries no `tool_use` block at
    // all — only the `<forked-skill-launch>` element on the local-command
    // system line. Folding that element must record the launch (so the later
    // `<task-notification>` correlates back) AND light a BACKGROUND indicator
    // (so it survives the degenerate turn end the same group emits).
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/example:review-pr"),
            forked_skill_launch_line("forked", "a7046b32df40e1b3e", "example:review-pr"),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("cmdname"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
            Effect::SubagentLaunched {
                tool_use_id: "forked-skill:a7046b32df40e1b3e".into(),
                thread_id: MAIN,
                // The forked skill knows its background-task id up front: the
                // payload's `agentId` IS the `<task-id>` its completion
                // notification will carry.
                task_id: Some("a7046b32df40e1b3e".into()),
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "forked-skill:a7046b32df40e1b3e".into(),
                thread_id: MAIN,
                subagent_type: Some("example:review-pr".into()),
                description: Some("/example:review-pr".into()),
                background: true,
            },
        ]
    );
    // The launch is seeded with its task_id, so a notification folded in the
    // SAME window matches by `<task-id>` without any store round-trip.
    assert_eq!(
        outcome
            .state
            .launched_threads
            .get("forked-skill:a7046b32df40e1b3e")
            .and_then(|launch| launch.task_id.clone()),
        Some("a7046b32df40e1b3e".into())
    );
    // The launch line is machinery: it never tears the turn off its thread.
    assert_eq!(outcome.state.carry_thread, MAIN);
}
