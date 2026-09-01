use delta_attribution::{attribute_lines, AttributionState, Effect, SubagentLaunch};

use crate::support::*;

#[test]
fn a_background_completion_seeded_from_an_earlier_window_lands_on_its_launch() {
    // The launch fell in an earlier sync window; only the persisted
    // `(toolu-bg -> CHILD)` map is reseeded (no launch line in this batch).
    // Carry is MAIN (the user has since moved on). The notification must still
    // resolve to CHILD via the seeded map, and only `SubagentCompleted` fires.
    let mut launched = std::collections::BTreeMap::new();
    launched.insert(
        "toolu-bg".to_owned(),
        SubagentLaunch {
            thread_id: CHILD,
            task_id: None,
        },
    );
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::with_launches(MAIN, None, launched),
        vec![task_notification_line_for("u-note", "toolu-bg")],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(
        outcome.effects,
        vec![Effect::SubagentCompleted {
            tool_use_id: "toolu-bg".into(),
        }]
    );
    assert!(outcome.state.launched_threads.is_empty());
}
