use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_keyless_task_notification_with_two_outstanding_launches_consumes_nothing() {
    // With several launches outstanding a keyless notification names none of
    // them and nothing distinguishes the one it reports. Guessing would clear
    // the wrong running indicator and file the continuation on the wrong
    // thread — worse than a stale indicator — so neither entry is consumed and
    // the line inherits `carry_thread`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::with_launches(
            CHILD,
            None,
            launches([("toolu-bg-1", MAIN), ("toolu-bg-2", CHILD)]),
        ),
        vec![
            task_notification_line("u-note"),
            assistant_line("a-after", "resuming"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert!(
        outcome.effects.is_empty(),
        "an ambiguous keyless notification completes nothing"
    );
    assert_eq!(
        outcome.state.launched_threads,
        launches([("toolu-bg-1", MAIN), ("toolu-bg-2", CHILD)]),
        "both launches stay outstanding"
    );
}
