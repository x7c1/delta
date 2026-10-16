use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn the_interrupt_marker_text_on_an_assistant_line_is_not_an_interrupt() {
    // Only a `role: user` line carrying the marker ends the turn. The same text
    // on an assistant line (the model quoting it, say) is ordinary output: it
    // must not surface a TurnInterrupted effect.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![assistant_line("a-quote", "[Request interrupted by user]")],
    );

    assert!(
        !outcome.effects.contains(&Effect::TurnInterrupted),
        "an assistant line is never the interrupt marker, got {:?}",
        outcome.effects
    );
}
