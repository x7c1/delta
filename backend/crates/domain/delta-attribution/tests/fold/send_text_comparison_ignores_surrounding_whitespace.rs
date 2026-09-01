use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn send_text_comparison_ignores_surrounding_whitespace() {
    // The text comparison no longer decides consumption (position does), but it
    // still decides the `attributed` flag — and it compares TRIMMED text, so a
    // send whose stored text carries surrounding whitespace is still recognized
    // as echoed verbatim and raises no rewrite warning.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, CHILD, "  spaced prompt \n"))),
        vec![user_line("u-1", "spaced prompt")],
    );

    assert_eq!(message(&outcome, "u-1").thread_id, CHILD);
    assert_eq!(
        outcome.effects,
        vec![Effect::SendMatched {
            send_id: 7,
            matched_uuid: MessageUuid::from("u-1"),
            attributed: true,
        }]
    );
}
