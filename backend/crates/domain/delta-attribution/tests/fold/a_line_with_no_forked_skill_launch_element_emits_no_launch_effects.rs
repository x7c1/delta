use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_line_with_no_forked_skill_launch_element_emits_no_launch_effects() {
    // The ordinary local-command group — a slash command that does NOT fork a
    // skill — must stay exactly as it was: a degenerate finished turn with no
    // subagent effects whatsoever.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            local_command_caveat_line("caveat", "pcmd"),
            local_command_name_line("cmdname", "pcmd", "/review-pr"),
            local_command_stdout_line("stdout", "pcmd"),
        ],
    );

    assert!(
        outcome.effects.is_empty(),
        "a local command with no forked-skill launch emits nothing, got {:?}",
        outcome.effects
    );
    assert!(outcome.state.launched_threads.is_empty());
}
