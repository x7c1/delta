use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn non_human_lines_inherit_carry() {
    // Meta (harness-injected), unclassified, and empty-text user lines are
    // not human turns: none of them resets or advances `carry_thread`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            meta_line("m-1", "<system-reminder>injected</system-reminder>"),
            other_line("o-1"),
            user_line("u-blank", "   "),
        ],
    );

    assert_eq!(
        threads(&outcome),
        vec![
            ("m-1".into(), CHILD),
            ("o-1".into(), CHILD),
            ("u-blank".into(), CHILD),
        ]
    );
    assert_eq!(outcome.state.carry_thread, CHILD);
}
