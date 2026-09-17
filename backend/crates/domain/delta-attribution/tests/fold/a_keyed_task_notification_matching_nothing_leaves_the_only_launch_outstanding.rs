use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_keyed_task_notification_matching_nothing_leaves_the_only_launch_outstanding() {
    // The guard on the single-outstanding rule: a notification that DOES
    // carry a key is not keyless, even when that key names no recorded
    // launch. The key is evidence the notification belongs to some other
    // launch (one from an earlier window, no longer seeded), so it must not
    // consume the unrelated launch that happens to be the only one
    // outstanding — doing so would clear a still-running subagent's indicator
    // and attribute the continuation to the wrong thread.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::with_launches(CHILD, None, launches([("toolu-bg", MAIN)])),
        vec![
            task_notification_line_with_task_id_only("u-note", "agent-unknown"),
            assistant_line("a-after", "resuming"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert!(
        outcome.effects.is_empty(),
        "a key that names no launch completes nothing"
    );
    assert_eq!(
        outcome.state.launched_threads,
        launches([("toolu-bg", MAIN)]),
        "the unrelated launch stays outstanding"
    );
}
