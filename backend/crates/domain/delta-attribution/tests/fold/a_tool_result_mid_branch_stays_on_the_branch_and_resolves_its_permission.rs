use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_tool_result_mid_branch_stays_on_the_branch_and_resolves_its_permission() {
    // Claude writes the `tool_result` as a `role: user` line; treating it as
    // a new human turn used to drop the rest of the turn onto `main`. (Ported
    // from the actor-level sync test
    // `tool_result_mid_branch_turn_stays_on_the_branch`.)
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(CHILD, None),
        vec![
            tool_use_line("a-call", "t1"),
            tool_result_line("u-res", "t1", false),
            assistant_line("a-final", "after the tool"),
        ],
    );

    assert_eq!(
        threads(&outcome),
        vec![
            ("a-call".into(), CHILD),
            ("u-res".into(), CHILD),
            ("a-final".into(), CHILD),
        ]
    );
    assert_eq!(
        outcome.effects,
        vec![Effect::ResolvePermission {
            tool_use_id: "t1".into(),
            allowed: true,
        }]
    );
}
