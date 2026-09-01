use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_task_output_retrieval_completes_the_background_launch_it_names() {
    // The parent retrieved the background task's result itself (`TaskOutput`
    // with `block: true`), so the harness injects NO `<task-notification>` —
    // the retrieval's own successful, `completed` result is the only signal
    // that the task is over. It correlates by `task_id` (a retrieval never
    // names the launching tool_use id) and must emit `SubagentCompleted` so
    // the running indicator clears.
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::new(MAIN, None),
        vec![
            background_tool_use_line("a-launch", "toolu-bg"),
            tool_result_with_agent_id_line("u-ack", "toolu-bg", "agent-xyz"),
            task_output_tool_use_line("a-read", "toolu-read", "agent-xyz"),
            task_output_result_line("u-read", "toolu-read", "agent-xyz", "completed", false),
        ],
    );

    assert!(
        outcome.effects.contains(&Effect::SubagentCompleted {
            tool_use_id: "toolu-bg".into(),
        }),
        "the retrieval completes the launch it read, got {:?}",
        outcome.effects
    );
    assert!(outcome.state.launched_threads.is_empty());
    // A retrieval is not a launch: it lights no indicator and records nothing.
    assert!(
        !outcome.effects.iter().any(|e| matches!(
            e,
            Effect::SubagentIndicatorStarted { tool_use_id, .. }
                | Effect::SubagentLaunched { tool_use_id, .. }
                if tool_use_id == "toolu-read"
        )),
        "a TaskOutput retrieval must never register as a launch, got {:?}",
        outcome.effects
    );
    // The carrier line attributes exactly as any `tool_result` does.
    assert_eq!(message(&outcome, "u-read").thread_id, MAIN);
}
