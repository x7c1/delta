//! Session rows: registration, activation, and listing. The history
//! aggregations over these rows live in `session_history`.

use delta_model::{AgentProvider, Session, SessionId, SessionStatus, Thread, ThreadId};

use crate::error::Result;
use crate::ports::{NewSession, SessionPageRow, SpawningSession};
use crate::SessionPageCursor;

use super::{FakeStore, FakeStoreInner};

/// A row's recency key: its last activity, falling back to the session's own
/// `created_at` when message-less — the `COALESCE(last_activity_at, created_at)`
/// the SQL queries sort on.
fn row_recency(row: &SessionPageRow) -> String {
    row.1.clone().unwrap_or_else(|| row.0.created_at.clone())
}

/// Every stored session as a `(session, last_activity_at)` row, ordered exactly
/// as the SQL session-list queries order them: recency DESC, `created_at` DESC,
/// `id` DESC. Every row is included, a message-less `spawning` one too (see
/// `SqliteStore::list_sessions_page`).
fn recency_ordered_rows(g: &FakeStoreInner) -> Vec<SessionPageRow> {
    let mut rows: Vec<SessionPageRow> = g
        .sessions
        .iter()
        .map(|s| {
            let last_activity_at = g
                .messages
                .iter()
                .filter(|m| m.session_id == s.id)
                .filter_map(|m| m.created_at.clone())
                .max();
            (s.clone(), last_activity_at)
        })
        .collect();
    rows.sort_by(|a, b| {
        row_recency(b)
            .cmp(&row_recency(a))
            .then_with(|| b.0.created_at.cmp(&a.0.created_at))
            .then_with(|| b.0.id.as_str().cmp(a.0.id.as_str()))
    });
    rows
}

impl FakeStore {
    pub(super) async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
        let mut g = self.inner.lock().unwrap();
        // Insert-if-absent, mirroring the real store's upsert: a re-registration
        // returns the existing session and its `main` thread, except that a
        // still-`spawning` row is activated (status flips, the hook-reported
        // transcript path is filled in).
        if let Some(session) = g.sessions.iter_mut().find(|s| s.id == new.id) {
            if session.status == SessionStatus::Spawning {
                session.status = SessionStatus::Active;
                session.cwd = new.cwd;
                session.transcript_path = Some(new.transcript_path);
            }
            let session = session.clone();
            let main_id = g
                .threads
                .iter()
                .find(|t| t.session_id == new.id && t.title == "main")
                .map(|t| t.id)
                .unwrap();
            return Ok((session, main_id));
        }
        let session = Session {
            id: new.id.clone(),
            cwd: new.cwd,
            transcript_path: Some(new.transcript_path),
            title: None,
            status: SessionStatus::Active,
            created_at: "2026-01-01T00:00:00Z".into(),
            // `register_session` is the external-claude / hook-activation
            // path: it never sees Delta's launch context, so the snapshot
            // stays unknown here. Delta-launched sessions record it via
            // `insert_spawning_session` instead, and that row already exists
            // when this activate path runs.
            branch_at_launch: new.branch_at_launch,
            repo_root: new.repo_root,
            // Same as the snapshot fields above: external-claude sessions have
            // no Delta-known launch dir to record. Worktree dirs cannot appear
            // here because external sessions don't go through worktree spawn.
            requested_workdir: None,
            repository_display_name: new.repository_display_name,
            // The hook-activation path is Claude Code only; a structured
            // provider never enters the store this way.
            provider: AgentProvider::Claude,
            provider_session_id: None,
            provider_thread_id: None,
            pull_request_number: None,
            failure_reason: None,
        };
        g.sessions.push(session.clone());
        g.next_thread_id += 1;
        let main_id = ThreadId(g.next_thread_id);
        g.threads.push(Thread {
            id: main_id,
            session_id: new.id,
            title: "main".into(),
            parent_thread_id: None,
            root_message_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            last_activity_at: None,
        });
        Ok((session, main_id))
    }

    pub(super) async fn insert_spawning_session(
        &self,
        spawning: SpawningSession<'_>,
    ) -> Result<(Session, ThreadId)> {
        let SpawningSession {
            id,
            cwd,
            branch_at_launch,
            repo_root,
            requested_workdir,
            repository_display_name,
            provider,
            pull_request_number,
        } = spawning;
        let mut g = self.inner.lock().unwrap();
        assert!(
            !g.sessions.iter().any(|s| &s.id == id),
            "insert_spawning_session must not be called for an existing id"
        );
        let session = Session {
            id: id.clone(),
            cwd: cwd.to_owned(),
            transcript_path: None,
            title: None,
            status: SessionStatus::Spawning,
            created_at: "2026-01-01T00:00:00Z".into(),
            branch_at_launch: branch_at_launch.map(str::to_owned),
            repo_root: repo_root.map(str::to_owned),
            requested_workdir: requested_workdir.map(str::to_owned),
            repository_display_name: repository_display_name.map(str::to_owned),
            provider,
            provider_session_id: None,
            provider_thread_id: None,
            pull_request_number,
            failure_reason: None,
        };
        g.sessions.push(session.clone());
        g.next_thread_id += 1;
        let main_id = ThreadId(g.next_thread_id);
        g.threads.push(Thread {
            id: main_id,
            session_id: id.clone(),
            title: "main".into(),
            parent_thread_id: None,
            root_message_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            last_activity_at: None,
        });
        Ok((session, main_id))
    }

    pub(super) async fn set_provider_ids(
        &self,
        id: &SessionId,
        provider_session_id: Option<&str>,
        provider_thread_id: Option<&str>,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(session) = g.sessions.iter_mut().find(|s| &s.id == id) {
            session.provider_session_id = provider_session_id.map(str::to_owned);
            session.provider_thread_id = provider_thread_id.map(str::to_owned);
            // Mirror the real store: recording the ids activates a still-spawning
            // row (the structured-provider analogue of first-hook activation).
            if session.status == SessionStatus::Spawning {
                session.status = SessionStatus::Active;
            }
        }
        Ok(())
    }

    pub(super) async fn delete_session(&self, id: &SessionId) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        // Mirror the real store's cascade: every child row goes with the
        // session.
        g.sessions.retain(|s| &s.id != id);
        g.threads.retain(|t| &t.session_id != id);
        g.sends.retain(|s| &s.session_id != id);
        g.messages.retain(|m| &m.session_id != id);
        g.permissions.retain(|p| &p.session_id != id);
        g.transcript_lines_read.remove(id);
        g.subagent_launches.retain(|(sid, _), _| sid != id);
        g.remembered_panes.remove(id);
        Ok(())
    }

    pub(super) async fn mark_session_failed(
        &self,
        id: &SessionId,
        reason: Option<&str>,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(session) = g
            .sessions
            .iter_mut()
            .find(|s| &s.id == id && s.status == SessionStatus::Spawning)
        {
            session.status = SessionStatus::Failed;
            session.failure_reason = reason.map(str::to_owned);
        }
        Ok(())
    }

    pub(super) async fn relocate_transcript(
        &self,
        id: &SessionId,
        transcript_path: &str,
        cwd: Option<&str>,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(session) = g.sessions.iter_mut().find(|s| &s.id == id) {
            session.transcript_path = Some(transcript_path.to_owned());
            if let Some(cwd) = cwd {
                session.cwd = cwd.to_owned();
            }
        }
        Ok(())
    }

    pub(super) async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<Vec<SessionPageRow>> {
        let g = self.inner.lock().unwrap();
        let mut rows = recency_ordered_rows(&g);
        // Apply the cursor: keep only rows strictly after it under the same
        // ordering.
        if let Some(c) = cursor {
            rows.retain(|row| {
                let r = row_recency(row);
                r < c.recency
                    || (r == c.recency
                        && (row.0.created_at < c.created_at
                            || (row.0.created_at == c.created_at
                                && row.0.id.as_str() < c.id.as_str())))
            });
        }
        rows.truncate(limit as usize);
        Ok(rows)
    }

    pub(super) async fn list_sessions_by_ids(
        &self,
        ids: &[SessionId],
    ) -> Result<Vec<SessionPageRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let g = self.inner.lock().unwrap();
        // Same recency order as the page query, filtered to the requested ids;
        // an id with no row simply matches nothing, mirroring the SQL `IN`.
        let mut rows = recency_ordered_rows(&g);
        rows.retain(|row| ids.contains(&row.0.id));
        Ok(rows)
    }

    pub(super) async fn session(&self, id: &SessionId) -> Result<Option<Session>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .sessions
            .iter()
            .find(|s| &s.id == id)
            .cloned())
    }
}
