//! The `kind` discriminants a [`WireSessionEvent`] can carry on the wire.

use super::WireSessionEvent;
use crate::agent_provider::WireAgentProvider;
use crate::rate_limit_window::WireRateLimitWindow;
use crate::status_snapshot::WireStatusSnapshot;

/// Every `kind` discriminant, in declaration order, as serde puts it on the
/// wire.
///
/// Derived by serializing one sample of each variant, so the strings come from
/// the same serde attributes that produce the actual frames — there is no
/// second, hand-maintained list to drift.
pub fn event_kinds() -> Vec<String> {
    sample_events()
        .iter()
        .map(|event| {
            serde_json::to_value(event)
                .expect("wire event serializes")
                .get("kind")
                .and_then(|kind| kind.as_str())
                .expect("wire event carries a string `kind` tag")
                .to_owned()
        })
        .collect()
}

/// One sample of every variant, in declaration order.
fn sample_events() -> Vec<WireSessionEvent> {
    // Exhaustiveness guard: adding a `WireSessionEvent` variant fails this
    // match until the new variant also gets a sample below.
    fn covered(event: &WireSessionEvent) {
        match event {
            WireSessionEvent::SessionRegistered { .. }
            | WireSessionEvent::SessionOpened { .. }
            | WireSessionEvent::SessionClosed { .. }
            | WireSessionEvent::SessionRemoved { .. }
            | WireSessionEvent::SendDispatched { .. }
            | WireSessionEvent::SendParked { .. }
            | WireSessionEvent::TurnStarted { .. }
            | WireSessionEvent::ExternalInput { .. }
            | WireSessionEvent::TurnCompleted { .. }
            | WireSessionEvent::TurnInterrupted { .. }
            | WireSessionEvent::TranscriptUpdated { .. }
            | WireSessionEvent::PermissionRequested { .. }
            | WireSessionEvent::QuestionAsked { .. }
            | WireSessionEvent::PermissionResolved { .. }
            | WireSessionEvent::SpawnPaneReady { .. }
            | WireSessionEvent::SpawnFailed { .. }
            | WireSessionEvent::AssistantStreaming { .. }
            | WireSessionEvent::SubagentStarted { .. }
            | WireSessionEvent::SubagentFinished { .. }
            | WireSessionEvent::StatusUpdated { .. }
            | WireSessionEvent::RepositoryCloneCompleted { .. }
            | WireSessionEvent::RepositoryCloneFailed { .. } => {}
        }
    }

    let session_id = || "sess-sample".to_owned();
    let samples = vec![
        WireSessionEvent::SessionRegistered {
            session_id: session_id(),
        },
        WireSessionEvent::SessionOpened {
            session_id: session_id(),
        },
        WireSessionEvent::SessionClosed {
            session_id: session_id(),
        },
        WireSessionEvent::SessionRemoved {
            session_id: session_id(),
        },
        WireSessionEvent::SendDispatched {
            session_id: session_id(),
            send_id: 1,
        },
        WireSessionEvent::SendParked {
            session_id: session_id(),
            send_id: 1,
            text: "never delivered".to_owned(),
        },
        WireSessionEvent::TurnStarted {
            session_id: session_id(),
            send_id: 1,
            thread_id: 1,
            matched_uuid: "uuid-sample".to_owned(),
        },
        WireSessionEvent::ExternalInput {
            session_id: session_id(),
            prompt: "prompt".to_owned(),
        },
        WireSessionEvent::TurnCompleted {
            session_id: session_id(),
            thread_id: Some(1),
            stop_reason: None,
        },
        WireSessionEvent::TurnInterrupted {
            session_id: session_id(),
            thread_id: Some(1),
        },
        WireSessionEvent::TranscriptUpdated {
            session_id: session_id(),
            thread_ids: vec![1],
        },
        WireSessionEvent::PermissionRequested {
            session_id: session_id(),
            request_id: 1,
            tool_name: "Bash".to_owned(),
            tool_input: "{\"command\":\"ls\"}".to_owned(),
            file_change: None,
            grant_root: None,
        },
        WireSessionEvent::QuestionAsked {
            session_id: session_id(),
            request_id: 1,
            thread_id: 1,
            tool_input: "{\"questions\":[]}".to_owned(),
        },
        WireSessionEvent::PermissionResolved {
            session_id: session_id(),
            request_id: 1,
        },
        WireSessionEvent::SpawnPaneReady {
            session_id: session_id(),
            pane_token: "delta-sample".to_owned(),
        },
        WireSessionEvent::SpawnFailed {
            session_id: session_id(),
            pane_token: Some("delta-sample".to_owned()),
            reason: Some("git error: worktree add failed".to_owned()),
            cancelled: false,
        },
        WireSessionEvent::AssistantStreaming {
            session_id: session_id(),
            thread_id: 1,
            message_id: "msg-sample".to_owned(),
            index: 0,
            is_final: false,
            delta: "chunk".to_owned(),
        },
        WireSessionEvent::SubagentStarted {
            session_id: session_id(),
            thread_id: 1,
            tool_use_id: "toolu-sample".to_owned(),
            subagent_type: Some("general-purpose".to_owned()),
            description: Some("Run ls".to_owned()),
            background: false,
        },
        WireSessionEvent::SubagentFinished {
            session_id: session_id(),
            tool_use_id: "toolu-sample".to_owned(),
        },
        WireSessionEvent::StatusUpdated {
            session_id: session_id(),
            // A populated sample rather than an empty one: the exhaustiveness
            // guard is what proves a new snapshot field reaches the generated
            // bindings, and an all-`null` sample would let a field be added
            // without its shape ever being exercised. The rate-limit window
            // carries its duration, which is the window's identity on the wire.
            snapshot: WireStatusSnapshot {
                provider: WireAgentProvider::Claude,
                model_id: Some("model-sample".to_owned()),
                model_display_name: Some("Model Sample".to_owned()),
                context_used_percentage: Some(42.5),
                context_window_size: Some(200_000),
                context_current_usage: Some(85_000),
                total_input_tokens: Some(90_000),
                rate_limits: Some(vec![WireRateLimitWindow {
                    duration_seconds: Some(5 * 60 * 60),
                    used_percentage: Some(12.0),
                    resets_at: Some(1_700_000_000),
                }]),
                total_cost_usd: Some(0.1234),
                current_dir: Some("/work".to_owned()),
            },
        },
        WireSessionEvent::RepositoryCloneCompleted {
            repo_owner: "x7c1".to_owned(),
            repo_name: "delta".to_owned(),
            clone_root: "/home/dev/projects".to_owned(),
            destination_path: "/home/dev/projects/delta".to_owned(),
        },
        WireSessionEvent::RepositoryCloneFailed {
            repo_owner: "x7c1".to_owned(),
            repo_name: "delta".to_owned(),
            clone_root: "/home/dev/projects".to_owned(),
            destination_path: "/home/dev/projects/delta".to_owned(),
            message: "could not resolve host github.com".to_owned(),
        },
    ];
    for event in &samples {
        covered(event);
    }
    samples
}
