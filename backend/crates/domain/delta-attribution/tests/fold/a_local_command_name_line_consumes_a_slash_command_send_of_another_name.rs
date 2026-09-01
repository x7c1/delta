use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn a_local_command_name_line_consumes_a_slash_command_send_of_another_name() {
    // Delta dispatched `/review-pr` as send 7, but the command-name line Claude
    // recorded names a DIFFERENT command (`/example:audit`) — a name rewrite
    // Delta has not catalogued, or something pre-empting the paste in the pane.
    // The send is still consumed: it was a slash command, so it produced no
    // `UserPromptSubmit` echo and no `Stop`, and this command line is the only
    // evidence it left. Deciding by name would leave send 7 wedged until the
    // echo deadline retyped the command a second time. The mismatch is
    // reported as `attributed: false` (the caller warns) rather than acted on.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/example:audit"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SendMatched {
                send_id: 7,
                matched_uuid: MessageUuid::from("cmdname"),
                attributed: false,
            },
            Effect::LocalCommandTurnEnded { send_id: 7 },
        ]
    );
    assert!(
        outcome.state.outstanding.is_empty(),
        "the command-name line consumed the outstanding slash-command send \
         despite naming another command"
    );
    assert_eq!(outcome.state.carry_thread, MAIN);
}
