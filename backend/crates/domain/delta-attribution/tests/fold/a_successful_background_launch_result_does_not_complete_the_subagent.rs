use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_successful_background_launch_result_does_not_complete_the_subagent() {
    // The negative: a modern `Agent` launch (no `run_in_background` key) whose
    // `tool_result` is the async launch acknowledgement is a normal background
    // launch. It must NOT be completed by the result — only its later
    // `<task-notification>` completes it — but the `agentId` the
    // acknowledgement carries is recorded on the launch for that notification
    // to match by.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            modern_agent_tool_use_line("a-launch", "toolu-bg", "Agent"),
            tool_result_with_agent_id_line("u-result", "toolu-bg", "a6a7d31c908cdfa24"),
        ],
    );

    assert!(
        !outcome
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SubagentCompleted { .. })),
        "an async launch acknowledgement must not complete the subagent, got {:?}",
        outcome.effects
    );
    // The launch survives for its `<task-notification>` to consume later.
    let launch = outcome
        .state
        .launched_threads
        .get("toolu-bg")
        .expect("the launch stays outstanding");
    assert_eq!(launch.task_id.as_deref(), Some("a6a7d31c908cdfa24"));
}
