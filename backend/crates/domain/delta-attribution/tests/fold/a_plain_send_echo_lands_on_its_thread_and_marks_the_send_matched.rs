use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn a_plain_send_echo_lands_on_its_thread_and_marks_the_send_matched() {
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "hello world"))),
        vec![
            user_line("u-1", "hello world"),
            assistant_line("a-1", "hi there"),
        ],
    );

    assert_eq!(message(&outcome, "u-1").thread_id, MAIN);
    assert_eq!(message(&outcome, "u-1").semantic_parent_uuid, None);
    assert_eq!(message(&outcome, "a-1").thread_id, MAIN);
    assert_eq!(
        outcome.effects,
        vec![Effect::SendMatched {
            send_id: 7,
            matched_uuid: MessageUuid::from("u-1"),
            attributed: true,
        }]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the match consumed the send"
    );
}
