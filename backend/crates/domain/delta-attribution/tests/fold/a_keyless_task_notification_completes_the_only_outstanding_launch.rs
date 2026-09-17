use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_keyless_task_notification_completes_the_only_outstanding_launch() {
    // A `<task-notification>` body carrying NEITHER `<tool-use-id>` nor
    // `<task-id>` names no launch — but with exactly one launch outstanding
    // there is nothing to be ambiguous about: the notification reports that
    // one. It takes the same path a keyed match takes — the entry is
    // consumed, `SubagentCompleted` clears the persisted correlation and the
    // running indicator, and the notification (plus the assistant
    // continuation it drives) lands on the LAUNCHING thread rather than
    // inheriting whatever thread happened to be current.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::with_launches(MAIN, None, launches([("toolu-bg", CHILD)])),
        vec![
            task_notification_line("u-note"),
            assistant_line("a-after", "resuming the task's thread"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert_eq!(
        outcome.effects,
        vec![Effect::SubagentCompleted {
            tool_use_id: "toolu-bg".into(),
        }]
    );
    assert!(outcome.state.launched_threads.is_empty());
}
