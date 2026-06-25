//! The wire form of [`Session`].

use delta_model::{Session, SessionStatus};
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of a session's lifecycle status.
///
/// Mirrors the domain [`SessionStatus`] variant-for-variant; this wire twin
/// carries the serialization concerns the domain type must not know about:
/// the lowercase variant names and the TypeScript export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "SessionStatus")]
pub enum WireSessionStatus {
    Spawning,
    Active,
    Ended,
    Failed,
}

impl From<SessionStatus> for WireSessionStatus {
    fn from(status: SessionStatus) -> Self {
        match status {
            SessionStatus::Spawning => WireSessionStatus::Spawning,
            SessionStatus::Active => WireSessionStatus::Active,
            SessionStatus::Ended => WireSessionStatus::Ended,
            SessionStatus::Failed => WireSessionStatus::Failed,
        }
    }
}

/// JSON shape of a session record on the REST surface.
///
/// Mirrors the domain [`Session`] field-for-field; see that type for the
/// semantics of each field. This wire twin carries the serialization concerns
/// the domain type must not know about: the field names on the wire and the
/// TypeScript export. Ids are plain `String` here because that is exactly what
/// crosses the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "Session")]
pub struct WireSession {
    pub id: String,
    pub cwd: String,
    /// Path of the session's JSONL transcript; empty while the session is
    /// still `spawning` (the domain stores `None` until the first hook reports
    /// the path, but the wire keeps the pre-existing string shape).
    pub transcript_path: String,
    pub title: Option<String>,
    pub status: WireSessionStatus,
    /// ISO-8601 timestamp.
    pub created_at: String,
    /// Spawn-time snapshot of the local git branch checked out in `cwd`.
    /// `null` when the launch directory was not inside a git repository, when
    /// HEAD was detached, or for sessions that predate this field. Never
    /// updated on resume or a later `git checkout`; the per-message
    /// `git_branch` is a separate per-turn snapshot.
    pub branch_at_launch: Option<String>,
    /// Spawn-time snapshot of the working-tree root containing `cwd`. `null`
    /// when the launch directory was not inside a git repository, or for
    /// sessions that predate this field. This is the working-tree path
    /// itself when the session was launched from a linked git worktree —
    /// see `repository_display_name` for the cross-worktree repository
    /// identity label.
    pub repo_root: Option<String>,
    /// Spawn-time short repository identity label (e.g. `org/repo`), derived
    /// from the launch directory's `origin` URL and falling back to the
    /// working-tree basename when no origin is configured. `null` when the
    /// launch directory was not inside a git repository, or for sessions that
    /// predate this field. The navigator renders this directly as the session
    /// card's repo line, falling back to the `cwd` basename when `null`.
    pub repository_display_name: Option<String>,
}

impl From<Session> for WireSession {
    fn from(session: Session) -> Self {
        WireSession {
            id: session.id.0,
            cwd: session.cwd,
            transcript_path: session.transcript_path.unwrap_or_default(),
            title: session.title,
            status: session.status.into(),
            created_at: session.created_at,
            branch_at_launch: session.branch_at_launch,
            repo_root: session.repo_root,
            repository_display_name: session.repository_display_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_model::SessionId;

    #[test]
    fn session_serializes_with_the_rest_field_names() {
        let session = Session {
            id: SessionId::from("sess-1"),
            cwd: "/work/delta".into(),
            transcript_path: Some("/tmp/t.jsonl".into()),
            title: None,
            status: SessionStatus::Active,
            created_at: "2026-01-01T00:00:00Z".into(),
            branch_at_launch: Some("main".into()),
            repo_root: Some("/work/delta".into()),
            requested_workdir: None,
            repository_display_name: Some("x7c1/delta".into()),
        };
        assert_eq!(
            serde_json::to_value(WireSession::from(session)).unwrap(),
            serde_json::json!({
                "id": "sess-1",
                "cwd": "/work/delta",
                "transcript_path": "/tmp/t.jsonl",
                "title": null,
                "status": "active",
                "created_at": "2026-01-01T00:00:00Z",
                "branch_at_launch": "main",
                "repo_root": "/work/delta",
                "repository_display_name": "x7c1/delta",
            }),
        );
    }

    #[test]
    fn every_status_serializes_lowercase() {
        for (status, expected) in [
            (SessionStatus::Spawning, "spawning"),
            (SessionStatus::Active, "active"),
            (SessionStatus::Ended, "ended"),
            (SessionStatus::Failed, "failed"),
        ] {
            assert_eq!(
                serde_json::to_value(WireSessionStatus::from(status)).unwrap(),
                serde_json::json!(expected),
            );
        }
    }
}
