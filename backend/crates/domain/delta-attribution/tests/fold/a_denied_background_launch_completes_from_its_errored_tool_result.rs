use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_denied_background_launch_completes_from_its_errored_tool_result() {
    // A background `Agent`/`Task` launch that the permission/auto-mode
    // classifier DENIES still writes its `tool_use` block to the parent JSONL,
    // so the running indicator lights. But the launch never happened, so no
    // `<task-notification>` will ever arrive to complete it, and the turn-end
    // sweep keeps background entries — the indicator would stay stuck forever.
    // The denial surfaces as an `is_error: true` `tool_result` for the same
    // `tool_use_id`; folding it must emit `SubagentCompleted` (alongside the
    // usual `ResolvePermission { allowed: false }`) and drop the launch entry.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            tool_result_line("u-result", "toolu-bg", true),
        ],
    );

    assert_eq!(
        outcome.effects,
        vec![
            Effect::SubagentLaunched {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                task_id: None,
            },
            Effect::SubagentIndicatorStarted {
                tool_use_id: "toolu-bg".into(),
                thread_id: MAIN,
                subagent_type: Some("general-purpose".into()),
                description: None,
                background: true,
            },
            Effect::ResolvePermission {
                tool_use_id: "toolu-bg".into(),
                allowed: false,
            },
            Effect::SubagentCompleted {
                tool_use_id: "toolu-bg".into(),
            },
        ],
        "a denied background launch must complete from its errored tool_result"
    );
    // The entry is drained so a later stray notification can't double-fire.
    assert!(outcome.state.launched_threads.is_empty());
}
