//! The wire form of [`Session`].

use delta_model::{AgentProvider, Session, SessionStatus};
use serde::{Deserialize, Serialize};
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

/// JSON shape of a session's AI-agent provider.
///
/// Mirrors the domain [`AgentProvider`] variant-for-variant; this wire twin
/// carries the serialization concerns the domain type must not know about: the
/// lowercase variant tokens (matching the persisted `session.provider` values)
/// and the TypeScript export the UI uses to render the provider badge.
///
/// Also accepted inbound (`Deserialize`) as the optional `provider` selector on
/// a new-session send, so the same token set names a provider in both
/// directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "AgentProvider")]
pub enum WireAgentProvider {
    Claude,
    Codex,
}

impl From<AgentProvider> for WireAgentProvider {
    fn from(provider: AgentProvider) -> Self {
        match provider {
            AgentProvider::Claude => WireAgentProvider::Claude,
            AgentProvider::Codex => WireAgentProvider::Codex,
        }
    }
}

impl From<WireAgentProvider> for AgentProvider {
    fn from(provider: WireAgentProvider) -> Self {
        match provider {
            WireAgentProvider::Claude => AgentProvider::Claude,
            WireAgentProvider::Codex => AgentProvider::Codex,
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
    /// Which AI-agent backend drives this session (`"claude"` / `"codex"`).
    /// The UI renders the provider badge from this; it is never used to branch
    /// behaviour.
    pub provider: WireAgentProvider,
    /// The provider's own conversation id, when the provider (not Delta) mints
    /// it (e.g. Codex's `thr_...`). `null` for a Claude session and for rows
    /// that predate provider persistence.
    pub provider_session_id: Option<String>,
    /// The provider's thread id. Currently equals `provider_session_id` for
    /// providers that map a session 1:1 onto a thread. `null` for Claude and
    /// for rows that predate provider persistence.
    pub provider_thread_id: Option<String>,
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
            provider: session.provider.into(),
            provider_session_id: session.provider_session_id,
            provider_thread_id: session.provider_thread_id,
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
            provider: AgentProvider::Claude,
            provider_session_id: None,
            provider_thread_id: None,
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
                "provider": "claude",
                "provider_session_id": null,
                "provider_thread_id": null,
            }),
        );
    }

    #[test]
    fn a_codex_session_serializes_its_provider_and_ids() {
        let session = Session {
            id: SessionId::from("sess-2"),
            cwd: "/work/delta".into(),
            transcript_path: None,
            title: None,
            status: SessionStatus::Active,
            created_at: "2026-01-01T00:00:00Z".into(),
            branch_at_launch: None,
            repo_root: None,
            requested_workdir: None,
            repository_display_name: None,
            provider: AgentProvider::Codex,
            provider_session_id: Some("thr_abc".into()),
            provider_thread_id: Some("thr_abc".into()),
        };
        let value = serde_json::to_value(WireSession::from(session)).unwrap();
        assert_eq!(value["provider"], serde_json::json!("codex"));
        assert_eq!(value["provider_session_id"], serde_json::json!("thr_abc"));
        assert_eq!(value["provider_thread_id"], serde_json::json!("thr_abc"));
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
