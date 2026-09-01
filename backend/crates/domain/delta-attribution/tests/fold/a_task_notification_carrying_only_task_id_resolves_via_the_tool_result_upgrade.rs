use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_task_notification_carrying_only_task_id_resolves_via_the_tool_result_upgrade() {
    // Recent Claude Code versions ship `<task-notification>` bodies with only
    // `<task-id>` (no `<tool-use-id>`). Without the fold-time upgrade the
    // task-id fallback finds no match — every launch entry's `task_id` is
    // still `None` because the live `PostToolUse(Agent)` hook did not run
    // during a cold-start replay — and the running indicator stays lit. The
    // tool_result of the launching tool_use carries `agentId: <id>` in its
    // human-readable text; the fold-time recovery captures it into the launch
    // entry's `task_id`, so the later notification correlates by task-id.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            tool_result_with_agent_id_line("u-result", "toolu-bg", "agent-xyz"),
            task_notification_line_with_task_id_only("u-note", "agent-xyz"),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                task_id: None,
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                subagent_type: Some("general-purpose".into()),
                description: None,
                background: true,
            },
            Effect::ResolvePermission {
                tool_use_id: "toolu-bg".into(),
                allowed: true,
            },
            Effect::SubagentCompleted {
                tool_use_id: "toolu-bg".into(),
            },
        ]
    );
    assert!(outcome.state.launched_threads.is_empty());
}
