use delta_attribution::{attribute_lines, AttributionState};

use crate::support::*;

#[test]
fn a_forked_skill_launch_with_an_unusable_body_emits_no_launch_effects() {
    // Both degenerate bodies: unparsable JSON, and JSON naming no `agentId`.
    // Without the correlation key nothing can be tracked, so the fold must
    // stay silent rather than mint a launch that can never be completed.
    for body in [
        "not json at all",
        r#"{"skillName":"example:review-pr"}"#,
        r#"{"agentId":""}"#,
    ] {
        let outcome = attribute_lines(
            &session(),
            MAIN,
            AttributionState::new(MAIN, None),
            vec![forked_skill_launch_line_with_body("forked", body)],
        );
        assert!(
            outcome.effects.is_empty(),
            "body {body:?} must emit no effects, got {:?}",
            outcome.effects
        );
        assert!(outcome.state.launched_threads.is_empty());
    }
}
