use delta_attribution::{attribute_lines, AttributionState};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn a_queued_command_matching_a_send_attributes_prompt_and_reply_to_the_child() {
    // LEGACY FORMAT (older claude versions; see the queued-prompt drift note
    // in docs/guides/development/canary.md): a branch send issued while a turn was
    // in flight was queued by Claude and recorded only as a `queued_command`
    // attachment — never a normal user line. It must still correlate to its
    // send so the prompt AND the reply land on the child thread, not `main`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(
            MAIN,
            Some(branch_send(7, CHILD, "uuid-parent", "branch text")),
        ),
        vec![
            queued_command_line("u-b", "branch text"),
            assistant_line("a-b", "branch reply"),
        ],
    );

    assert_eq!(message(&outcome, "u-b").thread_id, CHILD);
    assert_eq!(
        message(&outcome, "u-b").semantic_parent_uuid,
        Some(MessageUuid::from("uuid-parent"))
    );
    assert_eq!(message(&outcome, "a-b").thread_id, CHILD);
}
