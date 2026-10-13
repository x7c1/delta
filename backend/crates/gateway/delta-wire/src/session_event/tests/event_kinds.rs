//! The list of `kind` discriminants.

use super::*;

#[test]
fn event_kinds_lists_every_variant_in_declaration_order() {
    assert_eq!(
        event_kinds(),
        [
            "session_registered",
            "session_opened",
            "session_closed",
            "session_removed",
            "send_dispatched",
            "send_parked",
            "turn_started",
            "external_input",
            "turn_completed",
            "turn_interrupted",
            "transcript_updated",
            "permission_requested",
            "question_asked",
            "permission_resolved",
            "spawn_pane_ready",
            "spawn_failed",
            "assistant_streaming",
            "subagent_started",
            "subagent_finished",
            "status_updated",
            "repository_clone_completed",
            "repository_clone_failed",
        ],
    );
}
