//! An event the Interactor emits for the browser to render.

use serde::Serialize;

use delta_model::{MessageUuid, SessionId, ThreadId};

/// An event the Interactor emits for the browser to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEvent {
    /// The session was registered (first `UserPromptSubmit`).
    ///
    /// This doubles as the "opened" signal for a freshly-spawned session: a new
    /// spawn has no `session_id` until its first hook binds it, so its first
    /// liveness signal is this registration rather than a separate
    /// [`Self::SessionOpened`].
    SessionRegistered { session_id: SessionId },
    /// A known, previously-closed session became live again (resumed).
    ///
    /// Emitted when a session is reopened by id (e.g. `claude --resume`). A
    /// brand-new session never emits this — its first liveness signal is
    /// [`Self::SessionRegistered`]. Focus is purely client-side; this only says
    /// the session now has a live pane.
    SessionOpened { session_id: SessionId },
    /// An open session was closed: its pane was torn down but its data remains.
    SessionClosed { session_id: SessionId },
    /// A queued send was confirmed as a turn start.
    TurnStarted {
        session_id: SessionId,
        pending_send_id: i64,
        matched_uuid: MessageUuid,
    },
    /// External input was detected (typed directly into the pane).
    ExternalInput {
        session_id: SessionId,
        prompt: String,
    },
    /// A response completed.
    TurnCompleted {
        session_id: SessionId,
        stop_reason: Option<String>,
    },
    /// The in-flight turn was interrupted by the user (Escape / Ctrl-C).
    ///
    /// Detected from the transcript itself rather than a hook: when the user
    /// interrupts, Claude's `Stop` hook does not fire, so [`Self::TurnCompleted`]
    /// is never emitted and the optimistic "pending send" chip would stay
    /// "in progress" forever. The transcript tail instead sees a discrete
    /// `[Request interrupted by user...]` user line and emits this event, which
    /// clears the stuck pending send hook-independently (same delivery path as
    /// [`Self::PermissionResolved`]).
    TurnInterrupted { session_id: SessionId },
    /// The transcript grew between hooks (continuous tail).
    ///
    /// Emitted by the background poll when new lines were ingested, so the
    /// browser refetches the affected threads. Unlike [`Self::TurnCompleted`]
    /// and [`Self::ExternalInput`], this carries no turn semantics and must not
    /// mutate the pending-send FIFO or unread badges — it is a pure
    /// "refetch these threads" signal. `thread_ids` are the distinct threads of
    /// the newly-ingested messages.
    TranscriptUpdated {
        session_id: SessionId,
        thread_ids: Vec<ThreadId>,
    },
    /// A tool permission prompt is imminent.
    PermissionRequested {
        session_id: SessionId,
        request_id: i64,
        tool_name: String,
    },
    /// A previously-requested tool permission was resolved.
    ///
    /// Emitted when the `tool_result` correlated with an open
    /// [`Self::PermissionRequested`] is ingested. An auto-approved tool resolves
    /// almost immediately (the result lands right away), so the browser clears
    /// the notice promptly; a genuine TUI prompt yields no result until the
    /// human answers, so the notice persists until then.
    PermissionResolved {
        session_id: SessionId,
        request_id: i64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(event: &SessionEvent) -> serde_json::Value {
        serde_json::to_value(event).unwrap()
    }

    #[test]
    fn open_and_closed_serialize_as_id_routed_tagged_events() {
        assert_eq!(
            json(&SessionEvent::SessionOpened {
                session_id: SessionId::from("sess-1"),
            }),
            serde_json::json!({ "kind": "session_opened", "session_id": "sess-1" }),
        );
        assert_eq!(
            json(&SessionEvent::SessionClosed {
                session_id: SessionId::from("sess-1"),
            }),
            serde_json::json!({ "kind": "session_closed", "session_id": "sess-1" }),
        );
    }

    #[test]
    fn registered_keeps_its_wire_shape() {
        assert_eq!(
            json(&SessionEvent::SessionRegistered {
                session_id: SessionId::from("sess-1"),
            }),
            serde_json::json!({ "kind": "session_registered", "session_id": "sess-1" }),
        );
    }

    #[test]
    fn turn_interrupted_serializes_as_id_routed_tagged_event() {
        assert_eq!(
            json(&SessionEvent::TurnInterrupted {
                session_id: SessionId::from("sess-1"),
            }),
            serde_json::json!({ "kind": "turn_interrupted", "session_id": "sess-1" }),
        );
    }

    #[test]
    fn permission_requested_and_resolved_serialize_as_tagged_events() {
        assert_eq!(
            json(&SessionEvent::PermissionRequested {
                session_id: SessionId::from("sess-1"),
                request_id: 7,
                tool_name: "Bash".into(),
            }),
            serde_json::json!({
                "kind": "permission_requested",
                "session_id": "sess-1",
                "request_id": 7,
                "tool_name": "Bash",
            }),
        );
        assert_eq!(
            json(&SessionEvent::PermissionResolved {
                session_id: SessionId::from("sess-1"),
                request_id: 7,
            }),
            serde_json::json!({
                "kind": "permission_resolved",
                "session_id": "sess-1",
                "request_id": 7,
            }),
        );
    }
}
