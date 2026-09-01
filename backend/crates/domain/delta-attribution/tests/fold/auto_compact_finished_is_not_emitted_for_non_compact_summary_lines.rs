use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn auto_compact_finished_is_not_emitted_for_non_compact_summary_lines() {
    // Plain user / meta / assistant / other lines must not emit
    // `Effect::AutoCompactFinished` — only `Role::CompactSummary` is the
    // signal. Asserted on each non-compact-summary line individually so a
    // regression that fires the effect spuriously on any of them is caught.
    for line in [
        user_line("u-plain", "hello"),
        meta_line("m-1", "<system-reminder>noop</system-reminder>"),
        assistant_line("a-1", "ok"),
        other_line("o-1"),
    ] {
        let outcome = attribute_lines(
            &session(),
            MAIN,
            AttributionState::new(MAIN, None),
            vec![line],
        );
        assert!(
            !outcome
                .effects
                .iter()
                .any(|e| matches!(e, Effect::AutoCompactFinished)),
            "AutoCompactFinished must not fire for a non-compact-summary line: \
             got effects {:?}",
            outcome.effects
        );
    }
}
