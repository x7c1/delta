use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_denied_tool_result_resolves_its_permission_as_denied() {
    // A denied tool yields `is_error: true` ("User rejected tool use"); the
    // error flag infers allowed vs denied.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![tool_result_line("u-res", "t9", true)],
    );

    assert_eq!(
        outcome.effects,
        vec![Effect::ResolvePermission {
            tool_use_id: "t9".into(),
            allowed: false,
        }]
    );
}
