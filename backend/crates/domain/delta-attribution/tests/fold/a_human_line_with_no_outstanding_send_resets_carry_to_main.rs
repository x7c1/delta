use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_human_line_with_no_outstanding_send_resets_carry_to_main() {
    // The other half of the positional rule: with nothing outstanding, no send
    // can explain this line, so it is input typed straight into the pane. It
    // opens a turn on `main` and resets `carry_thread` — the reply follows it
    // to main — and no send effect is emitted.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            user_line("u-ext", "typed straight into the pane"),
            assistant_line("a-ext", "external reply"),
        ],
    );

    assert_eq!(message(&outcome, "u-ext").thread_id, MAIN);
    assert_eq!(message(&outcome, "a-ext").thread_id, MAIN);
    assert_eq!(outcome.state.carry_thread, MAIN);
    assert!(
        outcome.effects.is_empty(),
        "there was no send to consume, so nothing is reported as matched"
    );
}
