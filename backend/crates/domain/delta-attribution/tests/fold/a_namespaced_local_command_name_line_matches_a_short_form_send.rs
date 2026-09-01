use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn a_namespaced_local_command_name_line_matches_a_short_form_send() {
    // Like `a_local_command_group_folds_to_meta_resolves_its_send_and_ends_the_turn`,
    // but the user typed the SHORT form `/review-pr` (so Delta dispatched send
    // 7 with that exact text) while Claude expanded it
    // to its fully-qualified namespaced form `/example:review-pr` in the
    // transcript command-name line. Consumption is positional either way; what
    // this pins is that the bare-command-name comparison recognizes the two
    // forms as the same command, so the send is reported as `attributed` and
    // raises no rewrite warning.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/example:review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert_eq!(message(&outcome, "caveat").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "cmdname").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "stdout").role, delta_model::Role::Meta);

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("cmdname"),
                attributed: true,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the namespaced command-name line consumed the short-form outstanding send"
    );
    assert_eq!(outcome.state.carry_thread, MAIN);
}
