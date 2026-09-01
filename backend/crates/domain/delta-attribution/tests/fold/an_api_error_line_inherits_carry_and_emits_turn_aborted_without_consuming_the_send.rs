use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn an_api_error_line_inherits_carry_and_emits_turn_aborted_without_consuming_the_send() {
    // A synthetic `isApiErrorMessage` assistant line ends the turn on an API
    // error (a usage/session limit, a rate limit, ...). It is attributed like
    // any assistant line — inheriting `carry_thread`, never resetting to `main`
    // and never running through send correlation — and additionally surfaces a
    // `TurnAborted` effect, the turn-end signal that line carries in place of
    // the absent `Stop` hook / interrupt marker.
    let unrelated = send(9, MAIN, "unrelated prompt");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(unrelated.clone())),
        vec![api_error_line("e-1")],
    );

    assert_eq!(message(&outcome, "e-1").thread_id, CHILD);
    assert_eq!(outcome.effects, vec![Effect::TurnAborted]);
    assert_eq!(
        outcome.state.outstanding,
        vec![unrelated]
            .into_iter()
            .collect::<std::collections::VecDeque<_>>(),
        "the api-error line must not match or consume the pending send"
    );
}
