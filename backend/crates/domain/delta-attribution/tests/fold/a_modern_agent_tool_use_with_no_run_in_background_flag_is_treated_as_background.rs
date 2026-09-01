use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_modern_agent_tool_use_with_no_run_in_background_flag_is_treated_as_background() {
    // Modern Claude Code (>= v2.1.193) dropped the `run_in_background` parameter
    // from the `Agent`/`Task` tool schema and made these calls async by default.
    // An assistant line carrying such a tool_use — with no `run_in_background`
    // key at all — must still emit both `SubagentLaunched` (so a later
    // `<task-notification>` can correlate back to the launching thread) and
    // `SubagentIndicatorStarted { background: true }` (so the running indicator
    // SURVIVES the immediate `PostToolUse(Agent)` that fires when the launch
    // returned).
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![modern_agent_tool_use_line(
            "a-launch",
            "toolu-modern",
            "Agent",
        )],
    );

    assert_eq!(message(&outcome, "a-launch").thread_id, CHILD);
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "toolu-modern".into(),
                thread_id: CHILD,
                task_id: None,
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "toolu-modern".into(),
                thread_id: CHILD,
                subagent_type: Some("general-purpose".into()),
                description: Some("Run ls and count entries".into()),
                background: true,
            },
        ],
        "modern Agent shape (no flag) must launch as background and light a background indicator"
    );
    // The launch is recorded for a later `<task-notification>` to consume.
    assert!(outcome.state.launched_threads.contains_key("toolu-modern"));
}
