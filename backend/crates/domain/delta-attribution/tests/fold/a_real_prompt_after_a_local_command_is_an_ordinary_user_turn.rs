use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn a_real_prompt_after_a_local_command_is_an_ordinary_user_turn() {
    // The bare command-name line must not be confused with the human prompt
    // that follows it: a later user line with a DIFFERENT promptId is a genuine
    // turn (here matching its own send 8), proving the local-command grouping
    // is scoped to the caveat's promptId and does not leak forward.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState {
            outstanding: vec![send(7, MAIN, "/review-pr"), send(8, MAIN, "now review it")].into(),
            ..AttributionState::new(MAIN, None)
        },
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
            with_prompt_id("p-real", user_line("u-real", "now review it")),
        ],
    );

    // The local command consumed send 7 and ended that turn; the real prompt
    // consumed send 8 as an ordinary human turn.
    assert_eq!(message(&outcome, "u-real").role, delta_model::Role::User);
    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("cmdname"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
            Effect::SendMatched {
                send_id: 8,
                matched_uuid: MessageUuid::from("u-real"),
                attributed: true,
            },
        ]
    );
    assert!(outcome.state.outstanding.is_empty());
}
