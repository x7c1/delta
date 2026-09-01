use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn an_unknown_command_notice_matches_a_send_carrying_args() {
    // The dispatched send may carry args (`/review-pr 123`), while the notice
    // names only the command (`/review-pr`). The name comparison is on the
    // send's first whitespace-delimited token, so the args must not make the
    // send look unrecognized.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr 123"))),
        vec![unknown_command_notice_line("notice", "/review-pr")],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("notice"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(outcome.state.outstanding.is_empty());
}
