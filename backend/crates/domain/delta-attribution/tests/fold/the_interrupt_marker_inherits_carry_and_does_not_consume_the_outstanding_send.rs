use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn the_interrupt_marker_inherits_carry_and_does_not_consume_the_outstanding_send() {
    // The marker is a `role: user` line, but it belongs to the turn the user
    // just aborted: it must inherit `carry_thread` (not reset to `main`), must
    // not run through send correlation, and must surface as a TurnInterrupted
    // effect (Claude's `Stop` hook does not fire on interrupt).
    let unrelated = send(9, MAIN, "unrelated prompt");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(unrelated.clone())),
        vec![interrupt_line("u-int")],
    );

    assert_eq!(message(&outcome, "u-int").thread_id, CHILD);
    assert_eq!(outcome.effects, vec![Effect::TurnInterrupted]);
    assert_eq!(
        outcome.state.outstanding,
        vec![unrelated]
            .into_iter()
            .collect::<std::collections::VecDeque<_>>(),
        "the marker must not match or consume the pending send"
    );
}
