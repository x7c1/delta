use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_quoted_forked_skill_element_on_a_human_or_assistant_line_mints_no_launch() {
    // The payload is plain text, so a human prompt or an assistant reply can
    // QUOTE it — and in practice does: Delta is developed inside Delta, and its
    // own task docs and test fixtures carry a verbatim
    // `<forked-skill-launch>` element that a session working on this repo
    // routinely reads back into the transcript. Only the harness-written
    // `local_command` line (`Role::Meta` after parsing) may light an indicator:
    // a launch minted from a quote would be BACKGROUND, so it would survive
    // every turn-end sweep and stay lit until the session closed, with no
    // `<task-notification>` able to ever clear it.
    let quoted = "look at this line: <forked-skill-launch>\
                  {\"agentId\":\"a7046b32df40e1b3e\",\
                  \"skillName\":\"example:review-pr\"}</forked-skill-launch>";
    for line in [
        user_line("u-quote", quoted),
        assistant_line("a-quote", quoted),
    ] {
        let uuid = line.uuid.clone();
        let outcome = attribute_lines(
            &session(),
            MAIN,
            AttributionState::new(MAIN, None),
            vec![line],
        );
        assert!(
            outcome.effects.is_empty(),
            "a quoted element on {uuid:?} must emit no effects, got {:?}",
            outcome.effects
        );
        assert!(outcome.state.launched_threads.is_empty());
    }
}
