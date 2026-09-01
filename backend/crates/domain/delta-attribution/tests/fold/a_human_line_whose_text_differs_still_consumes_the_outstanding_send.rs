use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn a_human_line_whose_text_differs_still_consumes_the_outstanding_send() {
    // Consumption is POSITIONAL: while a send is outstanding its keystrokes are
    // already in the pane, so the next human user line is that send's echo
    // whatever text Claude Code recorded (a prompt rewrite, or characters typed
    // on top of the pasted text). It lands on the send's thread — together with
    // the reply that follows it — and the send is consumed. The text mismatch
    // survives only as `attributed: false`, the log-worthy hint that the echo
    // was not verbatim.
    let pending = branch_send(7, CHILD, "uuid-parent", "branch text");
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(pending)),
        vec![
            user_line("u-b", "branch text with extra characters"),
            assistant_line("a-b", "branch reply"),
        ],
    );

    assert_eq!(message(&outcome, "u-b").thread_id, CHILD);
    assert_eq!(
        message(&outcome, "u-b").semantic_parent_uuid,
        Some(MessageUuid::from("uuid-parent"))
    );
    // The reply follows the echo onto the branch instead of vanishing on main.
    assert_eq!(message(&outcome, "a-b").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
    assert_eq!(
        outcome.effects,
        vec![Effect::SendMatched {
            send_id: 7,
            matched_uuid: MessageUuid::from("u-b"),
            attributed: false,
        }]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the send is consumed by position, so it never lingers dispatched"
    );
}
