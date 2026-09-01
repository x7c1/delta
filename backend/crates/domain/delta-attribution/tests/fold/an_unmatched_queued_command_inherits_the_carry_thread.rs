use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn an_unmatched_queued_command_inherits_the_carry_thread() {
    // LEGACY FORMAT (older claude versions; see the queued-prompt drift note
    // in docs/guides/development/canary.md): a queued command that matches no send —
    // e.g. a background task notification injected mid-turn — is a
    // programmatic injection, not stray pane typing, so it must inherit the
    // active thread rather than reset attribution to `main`. (Ported from the
    // actor-level sync test
    // `unmatched_queued_command_mid_branch_stays_on_branch`.)
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            queued_command_line("u-note", "<task-notification>done</task-notification>"),
            assistant_line("a-after", "after the note"),
        ],
    );

    assert_eq!(message(&outcome, "u-note").thread_id, CHILD);
    assert_eq!(message(&outcome, "a-after").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert!(outcome.effects.is_empty());
}
