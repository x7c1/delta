use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn an_errored_result_for_a_non_launch_tool_emits_no_completion() {
    // The gate is precise: an `is_error: true` `tool_result` for a tool that
    // was NOT recorded as a background launch (e.g. an ordinary failed tool
    // never seeded into `launched_threads`) must resolve its permission but
    // must NOT emit a spurious `SubagentCompleted`.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            tool_use_line("a-tool", "toolu-plain"),
            tool_result_line("u-result", "toolu-plain", true),
        ],
    );

    assert!(
        !outcome
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SubagentCompleted { .. })),
        "an errored non-launch tool must not emit SubagentCompleted, got {:?}",
        outcome.effects
    );
    assert!(outcome
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ResolvePermission { allowed: false, .. })));
}
