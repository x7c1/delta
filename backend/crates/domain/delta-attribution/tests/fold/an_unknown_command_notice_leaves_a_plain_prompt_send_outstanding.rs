use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn an_unknown_command_notice_leaves_a_plain_prompt_send_outstanding() {
    // The same guard as on the local-command branch: send 7 is a PLAIN prompt,
    // so it is echoed through `UserPromptSubmit` and an unknown-command notice
    // cannot be its outcome — it is the rejection of a command typed straight
    // into the pane. Consuming send 7 here would drop the user's message, so
    // the notice merely surfaces and the send stays outstanding.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(send(7, CHILD, "hello world"))),
        vec![unknown_command_notice_line("notice", "/revew-pr")],
    );

    assert_eq!(message(&outcome, "notice").role, delta_model::Role::System);
    assert!(
        outcome.effects.is_empty(),
        "a plain-prompt send is neither consumed nor turn-ended by a notice"
    );
    assert_eq!(
        outcome.state.outstanding.len(),
        1,
        "the plain-prompt send stays outstanding, waiting for its own echo"
    );
    assert_eq!(
        outcome.state.carry_thread, CHILD,
        "the notice inherits the current thread, never resets to main"
    );
}
