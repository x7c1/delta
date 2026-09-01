use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_task_notification_carrying_only_tool_use_id_still_completes() {
    // Regression: when the notification body still carries `<tool-use-id>`
    // (the long-standing shape), the tool-use-id-keyed lookup must continue
    // to match. The fold-time upgrade runs harmlessly — it writes the
    // `task_id` onto the entry — but the completion resolves on the
    // tool-use-id path, exactly as before.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            tool_result_with_agent_id_line("u-result", "toolu-bg", "agent-xyz"),
            task_notification_line_with_tool_use_id_only("u-note", "toolu-bg"),
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
