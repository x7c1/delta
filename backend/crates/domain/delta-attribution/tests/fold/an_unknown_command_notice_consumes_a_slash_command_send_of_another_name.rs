use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn an_unknown_command_notice_consumes_a_slash_command_send_of_another_name() {
    // The unknown-notice analogue of
    // `a_local_command_name_line_consumes_a_slash_command_send_of_another_name`:
    // Delta dispatched `/review-pr 123` as send 7, and the notice names
    // `/revew-pr` — the shape this branch must expect, since Claude echoes back
    // whatever it parsed out of a command it did not recognize (extra
    // characters landing in the pane between Delta's paste and its Enter are
    // enough). The notice is still send 7's outcome, so it is consumed and the
    // degenerate turn ends; only `attributed` records that the names differ.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr 123"))),
        vec![unknown_command_notice_line("notice", "/revew-pr")],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("notice"),
                attributed: false,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the notice consumed the outstanding slash-command send despite naming \
         another command"
    );
}
