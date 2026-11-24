use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_background_bash_acknowledgement_leaves_its_launch_running() {
    // A background `Bash` also returns an immediate acknowledgement — text
    // without the subagent's `Async agent launched` marker — and is completed
    // later by its `<task-notification>`. Only a subagent tool's result is read
    // as a finished foreground run, so the acknowledgement must not complete it.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_bash_tool_use_line("a-launch", "toolu-bash"),
            tool_result_text_line(
                "u-result",
                "toolu-bash",
                "Command running in background with ID: bx7k2. Output is being written to: /tmp/bx7k2.output",
            ),
        ],
    );

    assert!(
        !outcome
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SubagentCompleted { .. })),
        "a background Bash acknowledgement must not complete it, got {:?}",
        outcome.effects
    );
    let launch = outcome
        .state
        .launched_threads
        .get("toolu-bash")
        .expect("the Bash launch stays outstanding for its notification");
    assert!(!launch.drives_indicator);
}
