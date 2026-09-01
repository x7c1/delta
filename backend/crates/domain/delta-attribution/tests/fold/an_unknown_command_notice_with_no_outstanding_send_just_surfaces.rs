use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn an_unknown_command_notice_with_no_outstanding_send_just_surfaces() {
    // An unknown command typed straight into the pane (never dispatched by
    // Delta): there is no outstanding send to resolve, so the notice surfaces as
    // a `Role::System` line with no send/turn effects — and must NOT reset
    // attribution to main.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![unknown_command_notice_line("notice", "/review-pr")],
    );

    assert_eq!(message(&outcome, "notice").role, delta_model::Role::System);
    assert!(
        outcome.effects.is_empty(),
        "nothing to resolve, nothing to end"
    );
    assert_eq!(
        outcome.state.carry_thread, CHILD,
        "the notice inherits the current thread, never resets to main"
    );
}
