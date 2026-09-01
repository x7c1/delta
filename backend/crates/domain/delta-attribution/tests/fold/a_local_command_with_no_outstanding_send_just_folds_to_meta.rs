use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_local_command_with_no_outstanding_send_just_folds_to_meta() {
    // A local command typed straight into the pane (never dispatched by Delta):
    // there is no outstanding send to resolve, so the group simply folds to
    // meta with no send/turn effects — and must NOT reset attribution to main.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert_eq!(message(&outcome, "cmdname").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "stdout").role, delta_model::Role::Meta);
    assert!(
        outcome.effects.is_empty(),
        "nothing to resolve, nothing to end"
    );
    assert_eq!(
        outcome.state.carry_thread, CHILD,
        "local-command machinery inherits the current thread, never resets to main"
    );
}
