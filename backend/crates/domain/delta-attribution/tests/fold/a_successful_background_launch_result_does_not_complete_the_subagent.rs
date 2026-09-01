use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_successful_background_launch_result_does_not_complete_the_subagent() {
    // The negative: a background launch whose `tool_result` is NOT an error
    // (`is_error: false`) is a normal async launch. It must NOT be completed by
    // the result — only its later `<task-notification>` completes it. The
    // errored-launch clear must fire strictly on `is_error: true`, so this
    // yields no `SubagentCompleted` and the launch entry survives, waiting for
    // its notification.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            tool_result_line("u-result", "toolu-bg", false),
        ],
    );

    assert!(
        !outcome
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SubagentCompleted { .. })),
        "a non-errored launch result must not complete the subagent, got {:?}",
        outcome.effects
    );
    // The launch survives for its `<task-notification>` to consume later.
    assert!(outcome.state.launched_threads.contains_key("toolu-bg"));
}
