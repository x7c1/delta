use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_background_task_completion_is_attributed_to_its_launching_thread() {
    // A background subagent is launched while the in-flight turn is on the
    // child thread (carry = CHILD). The launch records `(toolu-bg -> CHILD)`.
    // The user then moves to and works on `main` (carry resets to MAIN via an
    // external line). When the `<task-notification>` for `toolu-bg` finally
    // lands, it — and the assistant continuation it drives — must be attributed
    // to CHILD (the launching thread), NOT to MAIN (the current thread). Before
    // the correlation fix the notification blindly inherited `carry_thread` and
    // landed on MAIN.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            // The user leaves the sub-thread: an external human turn on main.
            user_line("u-ext", "now working on something else"),
            assistant_line("a-ext", "sure, on it"),
            // The background task completes; its notification correlates back.
            task_notification_line_for("u-note", "toolu-bg"),
            assistant_line("a-after", "the background agent finished"),
        ],
    );

    assert_eq!(message(&outcome, "a-launch").thread_id, CHILD);
    assert_eq!(message(&outcome, "u-ext").thread_id, MAIN);
    assert_eq!(message(&outcome, "a-ext").thread_id, MAIN);
    // The notification lands on the LAUNCHING thread, not the current one.
    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    // ...and the assistant's continuation of it follows onto that thread.
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);

    // The launch is recorded, the parent-transcript indicator effect fires for
    // the `Agent` tool_use, then the completion clears the launch.
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "toolu-bg".into(),
                thread_id: CHILD,
                task_id: None,
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "toolu-bg".into(),
                thread_id: CHILD,
                subagent_type: Some("general-purpose".into()),
                description: None,
                background: true,
            },
            Effect::SubagentCompleted {
                tool_use_id: "toolu-bg".into(),
            },
        ]
    );
    // The completion drained the correlation from the carried state.
    assert!(outcome.state.launched_threads.is_empty());
}
