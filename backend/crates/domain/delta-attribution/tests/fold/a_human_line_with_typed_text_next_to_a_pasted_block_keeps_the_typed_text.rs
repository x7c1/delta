use delta_attribution::{attribute_lines, AttributionState};
use delta_model::ContentBlock;

use crate::support::*;

#[test]
fn a_human_line_with_typed_text_next_to_a_pasted_block_keeps_the_typed_text() {
    // Someone typed straight into the pane and pasted a passage in the middle
    // of it. Only the wrapper Claude Code added around the paste goes — its
    // tags and the newlines it put around the body — while the typed text on
    // both sides and the pasted body stay.
    let recorded = "compare these two approaches:\n\n<pasted_content id=\"3b9c\">\nfirst approach\nsecond approach\n</pasted_content id=\"3b9c\">\n\nwhich one is simpler?";
    let expected =
        "compare these two approaches:first approach\nsecond approach\nwhich one is simpler?";

    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![user_line("u-ext", recorded)],
    );

    let line = message(&outcome, "u-ext");
    assert_eq!(line.content_text.as_deref(), Some(expected));
    assert_eq!(
        line.content,
        vec![ContentBlock::Text {
            text: expected.into()
        }]
    );
}

#[test]
fn a_malformed_pasted_block_on_a_human_line_is_stored_as_it_came() {
    // Mismatched ids: not a wrapper Delta recognizes, so nothing is removed.
    let recorded = "<pasted_content id=\"3b9c\">\nbody\n</pasted_content id=\"3b9d\">";

    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![user_line("u-ext", recorded)],
    );

    assert_eq!(
        message(&outcome, "u-ext").content_text.as_deref(),
        Some(recorded)
    );
}
