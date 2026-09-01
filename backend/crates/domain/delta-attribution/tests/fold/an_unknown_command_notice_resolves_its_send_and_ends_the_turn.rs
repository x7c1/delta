use delta_attribution::{attribute_lines, AttributionState, Effect};
use delta_model::MessageUuid;

use crate::support::*;

#[test]
fn an_unknown_command_notice_resolves_its_send_and_ends_the_turn() {
    // The user typed `/review-pr`, but no such slash command exists. Delta
    // dispatched it as send 7, so the turn machine is AwaitingEcho{7}. Claude
    // rejects an unknown command client-side: NO UserPromptSubmit echo, NO Stop,
    // and no command group — only a `system`/informational warning
    // "Unknown command: /review-pr". Left alone send 7 wedges the queue forever,
    // exactly like a known local command. The notice must consume send 7 and end
    // the degenerate turn, in that order.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, Some(send(7, MAIN, "/review-pr"))),
        vec![unknown_command_notice_line("notice", "/review-pr")],
    );

    // The notice surfaces as a system line (not folded to meta, not a user turn).
    assert_eq!(message(&outcome, "notice").role, delta_model::Role::System);
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
    assert!(
        outcome.state.outstanding.is_empty(),
        "the unknown-command notice consumed the outstanding send"
    );
    // The notice is machinery: it never tears the turn back off `main`.
    assert_eq!(outcome.state.carry_thread, MAIN);
}
