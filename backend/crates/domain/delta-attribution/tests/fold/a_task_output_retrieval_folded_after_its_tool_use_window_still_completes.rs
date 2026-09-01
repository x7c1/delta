use delta_attribution::{attribute_lines, AttributionState, Effect, SubagentLaunch};

use crate::support::*;

#[test]
fn a_task_output_retrieval_folded_after_its_tool_use_window_still_completes() {
    // The real ordering for a BLOCKING retrieval: the assistant's `TaskOutput`
    // `tool_use` line is flushed to the JSONL as soon as the message
    // completes, while its `tool_result` lands only when the task finishes —
    // routinely a later sync window, since the ambient tail polls throughout.
    // Nothing in-memory can bridge that gap, so the retrieval report's own
    // `<task_id>` must carry the correlation.
    let mut launched = std::collections::BTreeMap::new();
    launched.insert(
        "toolu-bg".to_owned(),
        SubagentLaunch {
            thread_id: CHILD,
            task_id: Some("agent-xyz".to_owned()),
        },
    );
    let outcome = attribute_lines(
        &session(),
        MAIN,
        AttributionState::with_launches(MAIN, None, launched),
        vec![task_output_result_line(
            "u-read",
            "toolu-read",
            "agent-xyz",
            "completed",
            false,
        )],
    );

    assert!(
        outcome.effects.contains(&Effect::SubagentCompleted {
            tool_use_id: "toolu-bg".into(),
        }),
        "a retrieval report alone still completes its launch, got {:?}",
        outcome.effects
    );
    assert!(outcome.state.launched_threads.is_empty());
    // Thread attribution is untouched: the carrier inherits `carry_thread`,
    // NOT the launching thread (that is the notification path's job).
    assert_eq!(message(&outcome, "u-read").thread_id, MAIN);
    assert_eq!(outcome.state.carry_thread, MAIN);
}
