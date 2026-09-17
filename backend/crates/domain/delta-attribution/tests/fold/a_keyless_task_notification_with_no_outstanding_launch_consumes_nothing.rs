use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_keyless_task_notification_with_no_outstanding_launch_consumes_nothing() {
    // With NO launch outstanding the single-outstanding rule has nothing to
    // resolve to (the launch fell in a window no longer seeded, or was
    // already completed by another signal). The keyless notification emits no
    // effect and inherits `carry_thread`, exactly as before the rule existed.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            task_notification_line("u-note"),
            assistant_line("a-after", "resuming"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert!(
        outcome.effects.is_empty(),
        "with no launch outstanding a keyless notification completes nothing"
    );
}
