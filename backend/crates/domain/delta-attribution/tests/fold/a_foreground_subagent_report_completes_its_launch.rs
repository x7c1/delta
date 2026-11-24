use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_foreground_subagent_report_completes_its_launch() {
    // A modern `Agent` tool_use carries no `run_in_background` key, so the fold
    // records it as a background launch and lights a background indicator. But
    // Claude Code decides per subagent type, and this one ran in the
    // foreground: its `tool_result` is the subagent's final report, not the
    // async launch acknowledgement. No `<task-notification>` will follow, so
    // the result itself completes the launch.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            modern_agent_tool_use_line("a-launch", "toolu-fg", "Agent"),
            foreground_subagent_report_line("u-result", "toolu-fg", "a1b2c3"),
        ],
    );

    assert!(
        outcome.effects.contains(&Effect::SubagentIndicatorStarted {
            tool_use_id: "toolu-fg".into(),
            thread_id: MAIN,
            subagent_type: Some("general-purpose".into()),
            description: Some("Run ls and count entries".into()),
            background: true,
        }),
        "the launch lit a background indicator, got {:?}",
        outcome.effects
    );
    assert!(
        outcome.effects.contains(&Effect::SubagentCompleted {
            tool_use_id: "toolu-fg".into(),
        }),
        "the foreground report completes the launch, got {:?}",
        outcome.effects
    );
    assert!(
        outcome.state.launched_threads.is_empty(),
        "the completed launch is dropped from launched_threads"
    );
}

#[test]
fn a_foreground_subagent_report_completes_a_launch_seeded_from_an_earlier_window() {
    // The usual timing: a foreground subagent runs for a while, so its report
    // lands in a later sync window than the launch line, and the launch reaches
    // this fold only through the persisted seed.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::with_launches(MAIN, None, launches([("toolu-fg", CHILD)])),
        vec![foreground_subagent_report_line(
            "u-result", "toolu-fg", "a1b2c3",
        )],
    );

    assert!(
        outcome.effects.contains(&Effect::SubagentCompleted {
            tool_use_id: "toolu-fg".into(),
        }),
        "the foreground report completes the seeded launch, got {:?}",
        outcome.effects
    );
    assert!(outcome.state.launched_threads.is_empty());
}
