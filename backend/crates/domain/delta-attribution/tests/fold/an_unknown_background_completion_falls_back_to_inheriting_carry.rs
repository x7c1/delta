use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn an_unknown_background_completion_falls_back_to_inheriting_carry() {
    // A `<task-notification>` whose `<tool-use-id>` is not in the launch map
    // (its launch fell in a window no longer seeded) must not regress: it
    // inherits `carry_thread` exactly as before, emits no completion effect,
    // and does not reset to `main`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            task_notification_line_for("u-note", "toolu-unknown"),
            assistant_line("a-after", "resuming"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert!(
        outcome.effects.is_empty(),
        "an uncorrelated task-notification emits no completion effect"
    );
}
