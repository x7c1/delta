use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_task_notification_mid_branch_inherits_carry_and_does_not_reset_to_main() {
    // A `<task-notification>` is a harness-injected background-task completion,
    // delivered as a plain `role: user` line (NOT a legacy `queued_command`
    // attachment). It is a programmatic continuation of the in-flight turn, not
    // a new human turn, so it must inherit `carry_thread` — the notification,
    // the assistant's continuation, and every later turn must stay on the
    // sub-thread rather than dropping onto `main`. It must also not run through
    // send correlation. (Regression: keying the inherit guard on
    // `is_queued_command`, which real task-notifications do not carry, let them
    // fall through to the `main` reset.)
    let unrelated = send(9, MAIN, "unrelated prompt");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(unrelated.clone())),
        vec![
            task_notification_line("u-note"),
            assistant_line("a-after", "resuming the sub-thread work"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert!(
        outcome.effects.is_empty(),
        "a task-notification neither resolves a permission nor matches a send"
    );
    assert_eq!(
        outcome.state.outstanding,
        vec![unrelated]
            .into_iter()
            .collect::<std::collections::VecDeque<_>>(),
        "the notification must not match or consume the pending send"
    );
}
