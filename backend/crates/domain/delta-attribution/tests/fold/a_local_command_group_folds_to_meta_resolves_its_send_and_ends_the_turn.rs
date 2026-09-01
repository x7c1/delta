use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn a_local_command_group_folds_to_meta_resolves_its_send_and_ends_the_turn() {
    // The user ran `/review-pr` as the session's first prompt. Delta dispatched
    // it as send 7, so the turn machine is AwaitingEcho{7}. Claude handles the
    // local command client-side: a 3-line group sharing one promptId (the
    // isMeta caveat, the bare command-name line, the stdout), and NO
    // UserPromptSubmit echo / NO Stop. The command-name and stdout lines must
    // fold to `Meta` (not render as user bubbles), send 7 must be consumed
    // against the command-name line, and the degenerate turn must end so the
    // send is freed and the machine can return to idle.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    // Every member of the group folds to meta (so the conversation pane
    // collapses them rather than showing user bubbles).
    assert_eq!(message(&outcome, "caveat").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "cmdname").role, delta_model::Role::Meta);
    assert_eq!(message(&outcome, "stdout").role, delta_model::Role::Meta);

    // The command-name line consumes the dispatched send and ends the turn, in
    // that order, so the caller marks the send matched before feeding the turn
    // machine the stop.
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
        "the local command consumed the outstanding send"
    );
    // The group is machinery: it never tears the turn back off `main`.
    assert_eq!(outcome.state.carry_thread, MAIN);
}
