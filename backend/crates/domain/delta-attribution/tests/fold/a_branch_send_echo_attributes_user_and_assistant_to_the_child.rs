use delta_attribution::{attribute_lines, AttributionState};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn a_branch_send_echo_attributes_user_and_assistant_to_the_child() {
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(
            MAIN,
            Some(branch_send(7, CHILD, "uuid-parent", "branch text")),
        ),
        vec![
            user_line("u-b", "branch text"),
            assistant_line("a-b", "branch reply"),
        ],
    );

    // The echo opens the child thread and carries the branch semantic parent.
    assert_eq!(message(&outcome, "u-b").thread_id, CHILD);
    assert_eq!(
        message(&outcome, "u-b").semantic_parent_uuid,
        Some(MessageUuid::from("uuid-parent"))
    );
    // The assistant reply carries forward to the child, not main.
    assert_eq!(message(&outcome, "a-b").thread_id, CHILD);
    assert_eq!(outcome.state.carry_thread, CHILD);
}
