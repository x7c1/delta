use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_forked_skill_launch_is_attributed_to_the_launching_thread() {
    // The launching thread is `carry_thread` — the thread the group's lines and
    // the later `<task-notification>` are attributed to — so the indicator, the
    // messages and the unread suppression all agree even when the command was
    // run from a sub-thread.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![forked_skill_launch_line(
            "forked",
            "agent-1",
            "example:review-pr",
        )],
    );

    assert_eq!(message(&outcome, "forked").thread_id, CHILD);
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "forked-skill:agent-1".into(),
                thread_id: CHILD,
                task_id: Some("agent-1".into()),
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "forked-skill:agent-1".into(),
                thread_id: CHILD,
                subagent_type: Some("example:review-pr".into()),
                description: Some("/example:review-pr".into()),
                background: true,
            },
        ]
    );
}
