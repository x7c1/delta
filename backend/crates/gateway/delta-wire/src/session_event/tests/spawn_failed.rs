//! A session whose launch failed.

use super::*;

/// The watchdog-shaped producers name no cause, so the `reason` key is
/// absent rather than `null`.
#[test]
fn spawn_failed_serializes_with_id_and_pane_token() {
    assert_eq!(
        json(&WireSessionEvent::SpawnFailed {
            session_id: "sess-1".into(),
            pane_token: Some("delta-1".into()),
            reason: None,
            cancelled: false,
        }),
        serde_json::json!({
            "kind": "spawn_failed",
            "session_id": "sess-1",
            "pane_token": "delta-1",
            "cancelled": false,
        }),
    );
}

/// An adapter-backed (Codex) launch has no pane, so the key is absent
/// rather than an invented name: the session id is what a client keys on.
#[test]
fn spawn_failed_omits_the_pane_token_for_an_adapter_backed_launch() {
    assert_eq!(
        json(&WireSessionEvent::SpawnFailed {
            session_id: "sess-1".into(),
            pane_token: None,
            reason: Some("agent error: codex is not installed".into()),
            cancelled: false,
        }),
        serde_json::json!({
            "kind": "spawn_failed",
            "session_id": "sess-1",
            "reason": "agent error: codex is not installed",
            "cancelled": false,
        }),
    );
}

/// A failed launch preparation *does* know why, and that message is the
/// only place it can still reach the user — the send was accepted long
/// before the failure, so there is no error response to carry it.
#[test]
fn spawn_failed_carries_the_launch_failure_reason_when_known() {
    assert_eq!(
        json(&WireSessionEvent::SpawnFailed {
            session_id: "sess-1".into(),
            pane_token: Some("delta-1".into()),
            reason: Some("git error: worktree add failed".into()),
            cancelled: false,
        }),
        serde_json::json!({
            "kind": "spawn_failed",
            "session_id": "sess-1",
            "pane_token": "delta-1",
            "reason": "git error: worktree add failed",
            "cancelled": false,
        }),
    );
}

/// A launch the user cancelled ends in the same frame as a broken one, so
/// `cancelled` is the only thing that tells them apart — a client that
/// words a requested cancel neutrally reads the flag, never the prose.
#[test]
fn spawn_failed_marks_a_launch_the_user_cancelled() {
    assert_eq!(
        json(&WireSessionEvent::SpawnFailed {
            session_id: "sess-1".into(),
            pane_token: Some("delta-1".into()),
            reason: Some("closed while starting".into()),
            cancelled: true,
        }),
        serde_json::json!({
            "kind": "spawn_failed",
            "session_id": "sess-1",
            "pane_token": "delta-1",
            "reason": "closed while starting",
            "cancelled": true,
        }),
    );
}
