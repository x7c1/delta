//! In-memory [`SessionStore`] fake backing the interactor use-case tests.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use async_trait::async_trait;
use delta_model::{
    LaunchOption, Message, MessageUuid, PermissionRequest, PermissionStatus, Role, Send,
    SendStatus, Session, SessionId, SessionStatus, Thread, ThreadId,
};

use crate::error::Result;
use crate::ports::{NewSession, SessionPageRow, SessionStore};
use crate::SessionPageCursor;

/// Derive a thread's `root_message_uuid` the way the SQL store does: the
/// `semantic_parent_uuid` of the thread's first semantically parented message,
/// falling back to its earliest semantically parented send.
fn derive_root_message_uuid(g: &FakeStoreInner, thread_id: ThreadId) -> Option<MessageUuid> {
    g.messages
        .iter()
        .filter(|m| m.thread_id == thread_id && m.semantic_parent_uuid.is_some())
        .min_by_key(|m| m.seq)
        .and_then(|m| m.semantic_parent_uuid.clone())
        .or_else(|| {
            g.sends
                .iter()
                .filter(|s| s.thread_id == thread_id && s.semantic_parent_uuid.is_some())
                .min_by_key(|s| s.id)
                .and_then(|s| s.semantic_parent_uuid.clone())
        })
}

#[derive(Default)]
pub(crate) struct FakeStoreInner {
    pub(crate) sessions: Vec<Session>,
    pub(crate) threads: Vec<Thread>,
    pub(crate) next_thread_id: i64,
    pub(crate) sends: Vec<Send>,
    pub(crate) next_send_id: i64,
    pub(crate) messages: Vec<Message>,
    pub(crate) permissions: Vec<PermissionRequest>,
    pub(crate) next_perm_id: i64,
    pub(crate) transcript_lines_read: HashMap<SessionId, usize>,
    pub(crate) launch_options: Vec<LaunchOption>,
    pub(crate) next_launch_option_id: i64,
    /// Outstanding background-task launches keyed by `(session_id,
    /// tool_use_id)`, mirroring the SQL `subagent_launch` table.
    pub(crate) subagent_launches: HashMap<(SessionId, String), ThreadId>,
}

#[derive(Default)]
pub(crate) struct FakeStore {
    pub(crate) inner: Mutex<FakeStoreInner>,
}

#[async_trait]
impl SessionStore for FakeStore {
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
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
        });
        Ok((session, main_id))
    }

    async fn insert_spawning_session(
        &self,
        id: &SessionId,
        cwd: &str,
    ) -> Result<(Session, ThreadId)> {
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
        });
        Ok((session, main_id))
    }

    async fn delete_session(&self, id: &SessionId) -> Result<()> {
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
        Ok(())
    }

    async fn mark_session_failed(&self, id: &SessionId) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(session) = g
            .sessions
            .iter_mut()
            .find(|s| &s.id == id && s.status == SessionStatus::Spawning)
        {
            session.status = SessionStatus::Failed;
        }
        Ok(())
    }

    async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<Vec<SessionPageRow>> {
        let g = self.inner.lock().unwrap();
        // Build (session, last_activity_at) rows, then order exactly as the
        // SQL page query does: recency DESC, created_at DESC, id DESC, where
        // recency = last_activity_at or the session's created_at fallback.
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
            // A `spawning` session that ingested nothing is excluded, mirroring
            // the SQL page query (see `SqliteStore::list_sessions_page`).
            .filter(|(s, _)| {
                s.status != SessionStatus::Spawning
                    || g.messages.iter().any(|m| m.session_id == s.id)
            })
            .collect();
        let recency = |row: &SessionPageRow| -> String {
            row.1.clone().unwrap_or_else(|| row.0.created_at.clone())
        };
        rows.sort_by(|a, b| {
            recency(b)
                .cmp(&recency(a))
                .then_with(|| b.0.created_at.cmp(&a.0.created_at))
                .then_with(|| b.0.id.as_str().cmp(a.0.id.as_str()))
        });
        // Apply the cursor: keep only rows strictly after it under the same
        // ordering.
        if let Some(c) = cursor {
            rows.retain(|row| {
                let r = recency(row);
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

    async fn session(&self, id: &SessionId) -> Result<Option<Session>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .sessions
            .iter()
            .find(|s| &s.id == id)
            .cloned())
    }

    async fn last_activity_at(&self, session_id: &SessionId) -> Result<Option<String>> {
        let g = self.inner.lock().unwrap();
        Ok(g.messages
            .iter()
            .filter(|m| &m.session_id == session_id)
            .filter_map(|m| m.created_at.clone())
            .max())
    }

    async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId> {
        let g = self.inner.lock().unwrap();
        Ok(g.threads
            .iter()
            .find(|t| &t.session_id == session_id && t.title == "main")
            .unwrap()
            .id)
    }

    async fn recent_workdirs(&self, limit: u32) -> Result<Vec<crate::ports::RecentWorkdir>> {
        let g = self.inner.lock().unwrap();
        // Per-session recency: latest message, else the session's created_at.
        // A cwd's recency is the max across its sessions. Then distinct cwds,
        // most recent first.
        let mut by_cwd: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for s in &g.sessions {
            let recency = g
                .messages
                .iter()
                .filter(|m| m.session_id == s.id)
                .filter_map(|m| m.created_at.clone())
                .max()
                .unwrap_or_else(|| s.created_at.clone());
            by_cwd
                .entry(s.cwd.clone())
                .and_modify(|cur| {
                    if recency > *cur {
                        *cur = recency.clone();
                    }
                })
                .or_insert(recency);
        }
        let mut rows: Vec<(String, Option<String>)> = by_cwd
            .into_iter()
            .map(|(cwd, recency)| (cwd, Some(recency)))
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        rows.truncate(limit as usize);
        Ok(rows)
    }

    async fn thread(&self, id: ThreadId) -> Result<Option<Thread>> {
        let g = self.inner.lock().unwrap();
        Ok(g.threads.iter().find(|t| t.id == id).cloned().map(|mut t| {
            t.root_message_uuid = derive_root_message_uuid(&g, t.id);
            t
        }))
    }

    async fn list_threads(&self, session_id: &SessionId) -> Result<Vec<Thread>> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<Thread> = g
            .threads
            .iter()
            .filter(|t| &t.session_id == session_id)
            .cloned()
            .map(|mut t| {
                t.root_message_uuid = derive_root_message_uuid(&g, t.id);
                t
            })
            .collect();
        out.sort_by_key(|t| t.id);
        Ok(out)
    }

    async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
    ) -> Result<Thread> {
        let mut g = self.inner.lock().unwrap();
        g.next_thread_id += 1;
        let thread = Thread {
            id: ThreadId(g.next_thread_id),
            session_id: session_id.clone(),
            title: title.to_owned(),
            parent_thread_id,
            // Derived on read (from the thread's branch send/message), mirroring
            // the real store; see `derive_root_message_uuid`.
            root_message_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        g.threads.push(thread.clone());
        Ok(thread)
    }

    async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        let mut g = self.inner.lock().unwrap();
        g.next_send_id += 1;
        let send = Send {
            id: g.next_send_id,
            session_id: session_id.clone(),
            thread_id,
            semantic_parent_uuid: semantic_parent_uuid.cloned(),
            text: text.to_owned(),
            locator_quote: locator_quote.map(str::to_owned),
            status: SendStatus::Dispatched,
            matched_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        g.sends.push(send.clone());
        Ok(send)
    }

    async fn enqueue_queued_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        let mut g = self.inner.lock().unwrap();
        g.next_send_id += 1;
        let send = Send {
            id: g.next_send_id,
            session_id: session_id.clone(),
            thread_id,
            semantic_parent_uuid: semantic_parent_uuid.cloned(),
            text: text.to_owned(),
            locator_quote: locator_quote.map(str::to_owned),
            status: SendStatus::Queued,
            matched_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        g.sends.push(send.clone());
        Ok(send)
    }

    async fn send(&self, id: i64) -> Result<Option<Send>> {
        let g = self.inner.lock().unwrap();
        Ok(g.sends.iter().find(|s| s.id == id).cloned())
    }

    async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        let g = self.inner.lock().unwrap();
        Ok(g.sends
            .iter()
            .filter(|s| &s.session_id == session_id && s.status == SendStatus::Queued)
            .min_by_key(|s| s.id)
            .cloned())
    }

    async fn open_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<Send> = g
            .sends
            .iter()
            .filter(|s| {
                &s.session_id == session_id
                    && matches!(s.status, SendStatus::Queued | SendStatus::Dispatched)
            })
            .cloned()
            .collect();
        out.sort_by_key(|s| s.id);
        Ok(out)
    }

    async fn promote_queued_send(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g.sends.iter_mut().find(|s| s.id == id) {
            s.status = SendStatus::Dispatched;
        }
        Ok(())
    }

    async fn requeue_send(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Dispatched)
        {
            s.status = SendStatus::Queued;
        }
        Ok(())
    }

    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        let g = self.inner.lock().unwrap();
        Ok(g.sends
            .iter()
            .filter(|s| &s.session_id == session_id && s.status == SendStatus::Dispatched)
            .min_by_key(|s| s.id)
            .cloned())
    }

    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g.sends.iter_mut().find(|s| s.id == id) {
            s.status = SendStatus::Matched;
            s.matched_uuid = Some(matched_uuid.clone());
        }
        Ok(())
    }

    async fn latest_user_thread(&self, session_id: &SessionId) -> Result<Option<ThreadId>> {
        let g = self.inner.lock().unwrap();
        Ok(g.messages
            .iter()
            .filter(|m| &m.session_id == session_id && matches!(m.role, Role::User))
            .max_by_key(|m| m.seq)
            .map(|m| m.thread_id))
    }

    async fn cancel_send(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g.sends.iter_mut().find(|s| s.id == id) {
            s.status = SendStatus::Cancelled;
        }
        Ok(())
    }

    async fn cancel_queued_send(&self, id: i64) -> Result<bool> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Queued)
        {
            s.status = SendStatus::Cancelled;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn upsert_messages(&self, messages: &[Message]) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        for m in messages {
            if let Some(existing) = g.messages.iter_mut().find(|e| e.uuid == m.uuid) {
                *existing = m.clone();
            } else {
                g.messages.push(m.clone());
            }
        }
        Ok(())
    }

    async fn message_count(&self, session_id: &SessionId) -> Result<usize> {
        let g = self.inner.lock().unwrap();
        Ok(g.messages
            .iter()
            .filter(|m| &m.session_id == session_id)
            .count())
    }

    async fn transcript_lines_read(&self, session_id: &SessionId) -> Result<usize> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .transcript_lines_read
            .get(session_id)
            .copied()
            .unwrap_or(0))
    }

    async fn set_transcript_lines_read(&self, session_id: &SessionId, lines: usize) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .transcript_lines_read
            .insert(session_id.clone(), lines);
        Ok(())
    }

    async fn thread_messages(&self, thread_id: ThreadId) -> Result<Vec<Message>> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<Message> = g
            .messages
            .iter()
            .filter(|m| m.thread_id == thread_id)
            .cloned()
            .collect();
        out.sort_by_key(|m| m.seq);
        Ok(out)
    }

    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: Option<&str>,
    ) -> Result<PermissionRequest> {
        let mut g = self.inner.lock().unwrap();
        g.next_perm_id += 1;
        let req = PermissionRequest {
            id: g.next_perm_id,
            session_id: session_id.clone(),
            tool_name: tool_name.to_owned(),
            tool_input_json: tool_input_json.to_owned(),
            tool_use_id: tool_use_id.map(str::to_owned),
            status: PermissionStatus::Pending,
            decision_reason: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            decided_at: None,
        };
        g.permissions.push(req.clone());
        Ok(req)
    }

    async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> Result<Option<PermissionRequest>> {
        let mut g = self.inner.lock().unwrap();
        let req = g
            .permissions
            .iter_mut()
            .find(|r| r.id == request_id && r.status == PermissionStatus::Pending);
        match req {
            Some(req) => {
                req.status = if allowed {
                    PermissionStatus::Allowed
                } else {
                    PermissionStatus::Denied
                };
                req.decided_at = Some("2026-01-01T00:00:00Z".into());
                Ok(Some(req.clone()))
            }
            None => Ok(None),
        }
    }

    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> Result<Vec<i64>> {
        let mut g = self.inner.lock().unwrap();
        // Mirror the SQL: settle the PreToolUse row matching `tool_use_id` AND
        // any pending dialog row (`tool_use_id: None`) for the same session.
        let mut resolved = Vec::new();
        for req in g.permissions.iter_mut().filter(|r| {
            &r.session_id == session_id
                && r.status == PermissionStatus::Pending
                && (r.tool_use_id.as_deref() == Some(tool_use_id) || r.tool_use_id.is_none())
        }) {
            req.status = if allowed {
                PermissionStatus::Allowed
            } else {
                PermissionStatus::Denied
            };
            req.decided_at = Some("2026-01-01T00:00:00Z".into());
            resolved.push(req.id);
        }
        Ok(resolved)
    }

    async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.subagent_launches
            .insert((session_id.clone(), tool_use_id.to_owned()), thread_id);
        Ok(())
    }

    async fn clear_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.subagent_launches
            .remove(&(session_id.clone(), tool_use_id.to_owned()));
        Ok(())
    }

    async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<BTreeMap<String, ThreadId>> {
        let g = self.inner.lock().unwrap();
        Ok(g.subagent_launches
            .iter()
            .filter(|((sid, _), _)| sid == session_id)
            .map(|((_, tool_use_id), thread_id)| (tool_use_id.clone(), *thread_id))
            .collect())
    }

    async fn list_launch_options(&self) -> Result<Vec<LaunchOption>> {
        let g = self.inner.lock().unwrap();
        // Newest first (descending id), mirroring the SQL store's ordering.
        let mut out = g.launch_options.clone();
        out.sort_by_key(|o| std::cmp::Reverse(o.id));
        Ok(out)
    }

    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
    ) -> Result<LaunchOption> {
        let mut g = self.inner.lock().unwrap();
        g.next_launch_option_id += 1;
        let option = LaunchOption {
            id: g.next_launch_option_id,
            label: label.map(str::to_owned),
            name: name.to_owned(),
            value: value.map(str::to_owned),
            default_enabled,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        g.launch_options.push(option.clone());
        Ok(option)
    }

    async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> Result<Option<LaunchOption>> {
        let mut g = self.inner.lock().unwrap();
        match g.launch_options.iter_mut().find(|o| o.id == id) {
            Some(option) => {
                option.default_enabled = default_enabled;
                Ok(Some(option.clone()))
            }
            None => Ok(None),
        }
    }

    async fn delete_launch_option(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.launch_options.retain(|o| o.id != id);
        Ok(())
    }
}
