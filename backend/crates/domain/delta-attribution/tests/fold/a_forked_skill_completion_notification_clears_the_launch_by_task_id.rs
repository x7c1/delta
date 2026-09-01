use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_forked_skill_completion_notification_clears_the_launch_by_task_id() {
    // The real forked-skill completion shape: a `<task-notification>` carrying
    // only `<task-id>` (the `agentId`), no `<tool-use-id>` — there never was a
    // tool_use. It must resolve against the launch seeded above and emit
    // `SubagentCompleted` keyed by the SAME synthetic id the indicator was lit
    // under, so the running entry is finished.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            forked_skill_launch_line("forked", "agent-1", "example:review-pr"),
            task_notification_line_with_task_id_only("u-note", "agent-1"),
            assistant_line("a-after", "the review landed"),
        ],
    );

    assert_eq!(
        outcome.effects.last(),
        Some(&Effect::SubagentCompleted {
            tool_use_id: "forked-skill:agent-1".into(),
        })
    );
    assert!(
        outcome.state.launched_threads.is_empty(),
        "the completion consumed the forked-skill launch"
    );
}
