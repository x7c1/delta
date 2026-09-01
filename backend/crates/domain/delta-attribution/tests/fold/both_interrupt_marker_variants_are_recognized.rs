use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn both_interrupt_marker_variants_are_recognized() {
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![user_line(
            "u-int",
            "[Request interrupted by user for tool use]",
        )],
    );

    assert_eq!(message(&outcome, "u-int").thread_id, CHILD);
    assert_eq!(outcome.effects, vec![Effect::TurnInterrupted]);
}
