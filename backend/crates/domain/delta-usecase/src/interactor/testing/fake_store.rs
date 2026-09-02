//! In-memory [`SessionStore`] fake backing the interactor use-case tests.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use async_trait::async_trait;
use delta_attribution::SubagentLaunch;
use delta_model::{
    AgentProvider, LaunchOption, Message, MessageUuid, PermissionRequest, PermissionStatus,
    PromptTemplate, Role, Send, SendStatus, Session, SessionId, SessionStatus, Thread, ThreadId,
};

use crate::error::{Error, Result};
use crate::ports::{
    CloneRoot, NewSession, RepositoryCloneRow, SessionPageRow, SessionStore, SpawningSession,
};
use crate::SessionPageCursor;

/// The creation timestamp every fake-created row carries. Fixed so assertions
/// can name it; the real store stamps the wall clock.
const FAKE_CREATED_AT: &str = "2026-01-01T00:00:00Z";

/// The timestamp a fake update re-stamps `updated_at` with. Distinct from
/// [`FAKE_CREATED_AT`] so a test can tell an edited row from an untouched one —
/// which the real store's second-resolution clock cannot guarantee within a
/// single test.
const FAKE_UPDATED_AT: &str = "2026-01-02T00:00:00Z";

/// The `held_at` stamp both hold producers write (the boot restore and the
/// echo-deadline park). Fixed for the same reason as [`FAKE_CREATED_AT`]: a
/// test asserts the marker's presence, never its value.
const HELD_AT: &str = "2026-01-01T00:00:00Z";

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
    pub(crate) prompt_templates: Vec<PromptTemplate>,
    pub(crate) next_prompt_template_id: i64,
    /// Outstanding background-task launches keyed by `(session_id,
    /// tool_use_id)`, mirroring the SQL `subagent_launch` table. The value is
    /// the `SubagentLaunch` carrying the launching thread plus the optional
    /// `task_id` learned via the `PostToolUse(Agent)` hook.
    pub(crate) subagent_launches: HashMap<(SessionId, String), SubagentLaunch>,
    pub(crate) clone_roots: Vec<CloneRoot>,
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

    async fn set_provider_ids(
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
            // Every row is listed, including a message-less `spawning` one,
            // mirroring the SQL page query (see `SqliteStore::list_sessions_page`).
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
        // Mirror the SQLite query: group on `COALESCE(requested_workdir, cwd)`
        // so worktree-managed paths drop out and legacy rows still surface by
        // their `cwd`. Per-session recency is the latest message, else the
        // session's `created_at`; a workdir's recency is the max across its
        // sessions.
        let mut by_workdir: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for s in &g.sessions {
            let recency = g
                .messages
                .iter()
                .filter(|m| m.session_id == s.id)
                .filter_map(|m| m.created_at.clone())
                .max()
                .unwrap_or_else(|| s.created_at.clone());
            let workdir = s.requested_workdir.clone().unwrap_or_else(|| s.cwd.clone());
            by_workdir
                .entry(workdir)
                .and_modify(|cur| {
                    if recency > *cur {
                        *cur = recency.clone();
                    }
                })
                .or_insert(recency);
        }
        let mut rows: Vec<(String, Option<String>)> = by_workdir
            .into_iter()
            .map(|(workdir, recency)| (workdir, Some(recency)))
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        rows.truncate(limit as usize);
        Ok(rows)
    }

    async fn cwd_exists(&self, path: &str) -> Result<bool> {
        // Mirror the SQLite UNION: match any session.cwd, session.requested_workdir,
        // or message.cwd equal to `path` (byte-for-byte).
        let g = self.inner.lock().unwrap();
        let hit_in_sessions = g
            .sessions
            .iter()
            .any(|s| s.cwd == path || s.requested_workdir.as_deref() == Some(path));
        if hit_in_sessions {
            return Ok(true);
        }
        let hit_in_messages = g.messages.iter().any(|m| m.cwd.as_deref() == Some(path));
        Ok(hit_in_messages)
    }

    async fn repository_clone_rows(
        &self,
        worktree_base: &str,
        active_repo_limit: i64,
        user_clone_limit: i64,
        generated_clone_limit: i64,
    ) -> Result<Vec<RepositoryCloneRow>> {
        let g = self.inner.lock().unwrap();
        // Mirror the SQL pipeline in pure Rust:
        //
        // 1. Group all sessions by `(repo_root, clone_path)` and keep the
        //    newest row per group. Recency is `last_activity_at` (the latest
        //    message's `created_at`) when set, else the session's `created_at`;
        //    ties break on insertion order (the fake's proxy for the real
        //    store's monotonic `id` DESC tie-break).
        // 2. Compute each `repo_root`'s recency as the max group recency, then
        //    take the top `active_repo_limit` by `(recency DESC, repo_root
        //    ASC)`.
        // 3. Within each retained `repo_root`, classify each row's kind by the
        //    `worktree_base + "/"` prefix and cap user/generated paths
        //    separately.
        // 4. Sort by `(recency DESC, repo_root ASC, clone_path ASC)`, matching
        //    the SQL ORDER BY.
        struct Acc {
            latest_recency: Option<String>,
            latest_branch: Option<String>,
            latest_index: i64,
        }
        let mut by_pair: std::collections::HashMap<(String, String), Acc> =
            std::collections::HashMap::new();
        for (index, s) in g.sessions.iter().enumerate() {
            let index = index as i64;
            let Some(repo_root) = &s.repo_root else {
                continue;
            };
            let clone_path = s.requested_workdir.clone().unwrap_or_else(|| s.cwd.clone());
            let recency = g
                .messages
                .iter()
                .filter(|m| m.session_id == s.id)
                .filter_map(|m| m.created_at.clone())
                .max()
                .or_else(|| Some(s.created_at.clone()));
            let key = (repo_root.clone(), clone_path);
            let entry = by_pair.entry(key).or_insert_with(|| Acc {
                latest_recency: recency.clone(),
                latest_branch: s.branch_at_launch.clone(),
                latest_index: index,
            });
            let beats = match (&recency, &entry.latest_recency) {
                (Some(a), Some(b)) if a > b => true,
                (Some(_), None) => true,
                (Some(a), Some(b)) if a == b && index > entry.latest_index => true,
                _ => false,
            };
            if beats {
                entry.latest_recency = recency;
                entry.latest_branch = s.branch_at_launch.clone();
                entry.latest_index = index;
            }
        }

        // The `latest` row set: one per `(repo_root, clone_path)`.
        let latest: Vec<RepositoryCloneRow> = by_pair
            .into_iter()
            .map(|((repo_root, clone_path), acc)| RepositoryCloneRow {
                repo_root,
                clone_path,
                last_opened_at: acc.latest_recency,
                last_branch: acc.latest_branch,
            })
            .collect();

        // Active repo selection: sort repo_roots by (max recency DESC,
        // repo_root ASC), keep the top `active_repo_limit`.
        let mut by_root: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();
        for row in &latest {
            let entry = by_root.entry(row.repo_root.clone()).or_insert(None);
            if row.last_opened_at > *entry {
                *entry = row.last_opened_at.clone();
            }
        }
        let mut root_recencies: Vec<(String, Option<String>)> = by_root.into_iter().collect();
        root_recencies.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let active_roots: std::collections::HashSet<String> = root_recencies
            .into_iter()
            .take(active_repo_limit.max(0) as usize)
            .map(|(root, _)| root)
            .collect();

        // Drop rows whose repo_root did not survive the active-repo cap, then
        // partition each surviving repo_root's rows by kind (`worktree_base +
        // "/"` prefix), sort each kind by (recency DESC, clone_path ASC), and
        // take its respective cap.
        let prefix = format!("{worktree_base}/");
        let mut surviving: Vec<RepositoryCloneRow> = latest
            .into_iter()
            .filter(|row| active_roots.contains(&row.repo_root))
            .collect();
        // Group by repo_root.
        let mut grouped: std::collections::HashMap<String, Vec<RepositoryCloneRow>> =
            std::collections::HashMap::new();
        for row in surviving.drain(..) {
            grouped.entry(row.repo_root.clone()).or_default().push(row);
        }
        let mut out: Vec<RepositoryCloneRow> = Vec::new();
        for (_, rows) in grouped {
            let (mut generated, mut user): (Vec<_>, Vec<_>) = rows
                .into_iter()
                .partition(|r| r.clone_path.starts_with(&prefix));
            let sort_rows = |v: &mut Vec<RepositoryCloneRow>| {
                v.sort_by(|a, b| {
                    b.last_opened_at
                        .cmp(&a.last_opened_at)
                        .then_with(|| a.clone_path.cmp(&b.clone_path))
                });
            };
            sort_rows(&mut user);
            sort_rows(&mut generated);
            user.truncate(user_clone_limit.max(0) as usize);
            generated.truncate(generated_clone_limit.max(0) as usize);
            out.extend(user);
            out.extend(generated);
        }

        // Final ORDER BY: (recency DESC, repo_root ASC, clone_path ASC).
        out.sort_by(|a, b| {
            b.last_opened_at
                .cmp(&a.last_opened_at)
                .then_with(|| a.repo_root.cmp(&b.repo_root))
                .then_with(|| a.clone_path.cmp(&b.clone_path))
        });
        Ok(out)
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
            held_at: None,
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
            held_at: None,
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
            .filter(|s| {
                &s.session_id == session_id
                    && s.status == SendStatus::Queued
                    // Held rows never dispatch automatically; they wait
                    // for an explicit release, mirroring the SQL filter.
                    && s.held_at.is_none()
            })
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

    async fn restore_all_dispatched(&self) -> Result<usize> {
        let mut g = self.inner.lock().unwrap();
        let mut restored = 0;
        for s in g
            .sends
            .iter_mut()
            .filter(|s| s.status == SendStatus::Dispatched)
        {
            s.status = SendStatus::Queued;
            s.held_at = Some(HELD_AT.into());
            restored += 1;
        }
        Ok(restored)
    }

    async fn hold_send_for_release(&self, id: i64) -> Result<bool> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Dispatched)
        {
            s.status = SendStatus::Queued;
            s.held_at = Some(HELD_AT.into());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn release_held_send(&self, id: i64) -> Result<bool> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Queued && s.held_at.is_some())
        {
            s.held_at = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        let g = self.inner.lock().unwrap();
        Ok(g.sends
            .iter()
            .filter(|s| &s.session_id == session_id && s.status == SendStatus::Dispatched)
            .min_by_key(|s| s.id)
            .cloned())
    }

    async fn dispatched_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<Send> = g
            .sends
            .iter()
            .filter(|s| &s.session_id == session_id && s.status == SendStatus::Dispatched)
            .cloned()
            .collect();
        out.sort_by_key(|s| s.id);
        Ok(out)
    }

    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g.sends.iter_mut().find(|s| s.id == id) {
            s.status = SendStatus::Matched;
            s.matched_uuid = Some(matched_uuid.clone());
        }
        Ok(())
    }

    async fn settle_send_delivered(&self, id: i64) -> Result<bool> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Dispatched)
        {
            // Delivered, but no transcript line claimed it: `matched_uuid`
            // stays `None`, exactly as the SQL leaves the column `NULL`.
            s.status = SendStatus::Matched;
            Ok(true)
        } else {
            Ok(false)
        }
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

    async fn deny_pending_permission_requests(
        &self,
        session_id: &SessionId,
        reason: &str,
    ) -> Result<Vec<i64>> {
        let mut g = self.inner.lock().unwrap();
        // Mirror the SQL: every still-pending row of the session becomes
        // `denied` carrying the reason, and only the ids that transitioned come
        // back.
        let mut denied = Vec::new();
        for req in g
            .permissions
            .iter_mut()
            .filter(|r| &r.session_id == session_id && r.status == PermissionStatus::Pending)
        {
            req.status = PermissionStatus::Denied;
            req.decision_reason = Some(reason.to_owned());
            req.decided_at = Some("2026-01-01T00:00:00Z".into());
            denied.push(req.id);
        }
        Ok(denied)
    }

    async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        // Mirrors the SQL UPSERT: the `task_id` of an already-upgraded row is
        // preserved across a re-record (the launching thread refreshes; the
        // separately-learned task id does not). A brand-new row starts with
        // `task_id: None` until `upgrade_subagent_task_id` runs.
        let key = (session_id.clone(), tool_use_id.to_owned());
        let task_id = g
            .subagent_launches
            .get(&key)
            .and_then(|l| l.task_id.clone());
        g.subagent_launches
            .insert(key, SubagentLaunch { thread_id, task_id });
        Ok(())
    }

    async fn upgrade_subagent_task_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        task_id: &str,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(launch) = g
            .subagent_launches
            .get_mut(&(session_id.clone(), tool_use_id.to_owned()))
        {
            launch.task_id = Some(task_id.to_owned());
        }
        Ok(())
    }

    async fn clear_subagent_launch(&self, session_id: &SessionId, tool_use_id: &str) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.subagent_launches
            .remove(&(session_id.clone(), tool_use_id.to_owned()));
        Ok(())
    }

    async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<BTreeMap<String, SubagentLaunch>> {
        let g = self.inner.lock().unwrap();
        Ok(g.subagent_launches
            .iter()
            .filter(|((sid, _), _)| sid == session_id)
            .map(|((_, tool_use_id), launch)| (tool_use_id.clone(), launch.clone()))
            .collect())
    }

    async fn list_launch_options(&self) -> Result<Vec<LaunchOption>> {
        let g = self.inner.lock().unwrap();
        // Delta-shipped rows first (ascending id), then the user's own newest
        // first, mirroring the SQL store's ordering.
        let mut out = g.launch_options.clone();
        out.sort_by_key(|o| match &o.builtin_key {
            Some(_) => (0, o.id),
            None => (1, -o.id),
        });
        Ok(out)
    }

    async fn launch_option(&self, id: i64) -> Result<Option<LaunchOption>> {
        let g = self.inner.lock().unwrap();
        Ok(g.launch_options.iter().find(|o| o.id == id).cloned())
    }

    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        let mut g = self.inner.lock().unwrap();
        g.next_launch_option_id += 1;
        let option = LaunchOption {
            id: g.next_launch_option_id,
            label: label.map(str::to_owned),
            name: name.to_owned(),
            value: value.map(str::to_owned),
            default_enabled,
            created_at: FAKE_CREATED_AT.into(),
            provider,
            builtin_key: None,
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

    async fn upsert_builtin_launch_option(
        &self,
        builtin_key: &str,
        label: &str,
        name: &str,
        value: Option<&str>,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        let mut g = self.inner.lock().unwrap();
        if let Some(existing) = g
            .launch_options
            .iter_mut()
            .find(|o| o.builtin_key.as_deref() == Some(builtin_key))
        {
            // The declared catalog owns these three; `default_enabled`,
            // `created_at` and the id stay exactly as they are.
            existing.label = Some(label.to_owned());
            existing.name = name.to_owned();
            existing.value = value.map(str::to_owned);
            existing.provider = provider;
            return Ok(existing.clone());
        }
        g.next_launch_option_id += 1;
        let option = LaunchOption {
            id: g.next_launch_option_id,
            label: Some(label.to_owned()),
            name: name.to_owned(),
            value: value.map(str::to_owned),
            // Offered, never imposed: a freshly materialized preset starts off.
            default_enabled: false,
            created_at: FAKE_CREATED_AT.into(),
            provider,
            builtin_key: Some(builtin_key.to_owned()),
        };
        g.launch_options.push(option.clone());
        Ok(option)
    }

    async fn delete_builtin_launch_options_except(&self, keys: &[&str]) -> Result<usize> {
        let mut g = self.inner.lock().unwrap();
        let before = g.launch_options.len();
        g.launch_options.retain(|o| match &o.builtin_key {
            // The user's own rows are never in scope of a catalog change.
            None => true,
            Some(key) => keys.contains(&key.as_str()),
        });
        Ok(before - g.launch_options.len())
    }

    async fn list_prompt_templates(&self) -> Result<Vec<PromptTemplate>> {
        let g = self.inner.lock().unwrap();
        // Oldest first (ascending created_at, then id), mirroring the SQL store.
        let mut out = g.prompt_templates.clone();
        out.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(out)
    }

    async fn create_prompt_template(&self, label: &str, text: &str) -> Result<PromptTemplate> {
        let mut g = self.inner.lock().unwrap();
        g.next_prompt_template_id += 1;
        let template = PromptTemplate {
            id: g.next_prompt_template_id,
            label: label.to_owned(),
            text: text.to_owned(),
            created_at: FAKE_CREATED_AT.to_owned(),
            updated_at: FAKE_CREATED_AT.to_owned(),
        };
        g.prompt_templates.push(template.clone());
        Ok(template)
    }

    async fn update_prompt_template(
        &self,
        id: i64,
        label: &str,
        text: &str,
    ) -> Result<Option<PromptTemplate>> {
        let mut g = self.inner.lock().unwrap();
        match g.prompt_templates.iter_mut().find(|t| t.id == id) {
            Some(template) => {
                template.label = label.to_owned();
                template.text = text.to_owned();
                template.updated_at = FAKE_UPDATED_AT.to_owned();
                Ok(Some(template.clone()))
            }
            None => Ok(None),
        }
    }

    async fn delete_prompt_template(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.prompt_templates.retain(|t| t.id != id);
        Ok(())
    }

    async fn list_clone_roots(&self) -> Result<Vec<CloneRoot>> {
        let g = self.inner.lock().unwrap();
        // Newest first (descending created_at), mirroring the SQL store. Ties
        // on the seeded timestamp fall back to path ASC for a deterministic order.
        let mut out = g.clone_roots.clone();
        out.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.path.cmp(&b.path))
        });
        Ok(out)
    }

    async fn insert_clone_root(&self, path: &str) -> Result<CloneRoot> {
        let mut g = self.inner.lock().unwrap();
        if g.clone_roots.iter().any(|r| r.path == path) {
            return Err(Error::CloneRootDuplicate(path.to_owned()));
        }
        let row = CloneRoot {
            path: path.to_owned(),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        g.clone_roots.push(row.clone());
        Ok(row)
    }

    async fn delete_clone_root(&self, path: &str) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.clone_roots.retain(|r| r.path != path);
        Ok(())
    }
}
