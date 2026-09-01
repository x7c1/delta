use delta_attribution::{attribute_lines, AttributionState, Effect};

use crate::support::*;

#[test]
fn a_running_or_errored_task_output_retrieval_leaves_the_launch_running() {
    // A non-blocking poll of a task still working (`<status>running</status>`)
    // and a retrieval that itself failed (`is_error: true`) both say nothing
    // about the task being over: the launch — and its running indicator —
    // must survive.
    for (uuid, status, is_error) in [
        ("u-poll", "running", false),
        ("u-failed-read", "completed", true),
    ] {
        let outcome = attribute_lines(
            &session(),
            MAIN,
            AttributionState::new(MAIN, None),
            vec![
                background_tool_use_line("a-launch", "toolu-bg"),
                tool_result_with_agent_id_line("u-ack", "toolu-bg", "agent-xyz"),
                task_output_tool_use_line("a-read", "toolu-read", "agent-xyz"),
                task_output_result_line(uuid, "toolu-read", "agent-xyz", status, is_error),
            ],
        );

        assert!(
            !outcome
                .effects
                .iter()
                .any(|e| matches!(e, Effect::SubagentCompleted { .. })),
            "status={status} is_error={is_error} must not complete the launch, got {:?}",
            outcome.effects
        );
        assert!(
            outcome.state.launched_threads.contains_key("toolu-bg"),
            "status={status} is_error={is_error} leaves the launch recorded"
        );
    }
}
