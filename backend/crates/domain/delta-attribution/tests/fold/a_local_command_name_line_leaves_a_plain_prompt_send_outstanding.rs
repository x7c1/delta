use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_local_command_name_line_leaves_a_plain_prompt_send_outstanding() {
    // The guard the positional rule needs: send 7 is a PLAIN prompt, which
    // Claude echoes back through `UserPromptSubmit` — so a local-command group
    // showing up while it is outstanding cannot be its outcome. Somebody typed
    // a command straight into the pane ahead of the send. Consuming send 7 here
    // would mark the user's message delivered and drop it, so the group folds
    // to meta and leaves the send outstanding for its own echo.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, Some(send(7, CHILD, "hello world"))),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert_eq!(message(&outcome, "cmdname").role, delta_model::Role::Meta);
    assert!(
        outcome.effects.is_empty(),
        "a plain-prompt send is neither consumed nor turn-ended by a command line"
    );
    assert_eq!(
        outcome.state.outstanding.len(),
        1,
        "the plain-prompt send stays outstanding, waiting for its own echo"
    );
    assert_eq!(
        outcome.state.carry_thread, CHILD,
        "local-command machinery inherits the current thread, never resets to main"
    );
}
