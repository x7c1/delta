//! Interactor use-case tests against in-memory fakes.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use delta_model::{
    ContentBlock, Message, MessageUuid, PendingSend, PendingSendStatus, PermissionRequest,
    PermissionStatus, Role, Session, SessionId, SessionStatus, Thread, ThreadId,
};

use crate::error::Result;
use crate::ports::{
    NewSession, SessionEvent, SessionLifecycle, SessionPageRow, SessionStore, TmuxDriver,
    Transcript, TranscriptMessage, TranscriptRead, UserPromptSubmitHook, Workspace,
};
use crate::{Interactor, SendTarget, SessionPageCursor};

/// A plain send into an existing thread.
fn to(thread_id: ThreadId) -> SendTarget {
    SendTarget::Thread {
        thread_id,
        branch_from: None,
    }
}

/// A branch send: the first message of a new branch off `parent`, hanging off
/// `thread_id` as the parent thread.
fn branch_off(thread_id: ThreadId, parent: &MessageUuid) -> SendTarget {
    SendTarget::Thread {
        thread_id,
        branch_from: Some(parent.clone()),
    }
}

/// A single recorded `create_session` call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CreatedSession {
    name: String,
    workdir: String,
    command: Vec<String>,
}

#[derive(Default)]
struct FakeTmux {
    /// The `(pane, text)` pairs `send_line` was called with.
    sent: Mutex<Vec<(String, String)>>,
    /// The panes `clear_input` was called with, in order.
    cleared: Mutex<Vec<String>>,
    /// When set, `send_line` fails instead of recording the line, simulating a
    /// dispatch failure into the pane.
    fail: bool,
    /// The session names currently "existing" for `has_session`.
    live: Mutex<Vec<String>>,
    /// The sessions `create_session` was called with, in order.
    created: Mutex<Vec<CreatedSession>>,
    /// The session names `kill_session` was called with, in order.
    killed: Mutex<Vec<String>>,
}

#[async_trait]
impl TmuxDriver for FakeTmux {
    async fn has_session(&self, name: &str) -> Result<bool> {
        Ok(self.live.lock().unwrap().iter().any(|n| n == name))
    }

    async fn create_session(&self, name: &str, workdir: &str, command: &[String]) -> Result<()> {
        self.created.lock().unwrap().push(CreatedSession {
            name: name.to_owned(),
            workdir: workdir.to_owned(),
            command: command.to_vec(),
        });
        self.live.lock().unwrap().push(name.to_owned());
        Ok(())
    }

    async fn send_line(&self, pane: &str, text: &str) -> Result<()> {
        if self.fail {
            return Err(crate::error::Error::Tmux("dispatch failed".into()));
        }
        self.sent
            .lock()
            .unwrap()
            .push((pane.to_owned(), text.to_owned()));
        Ok(())
    }

    async fn clear_input(&self, pane: &str) -> Result<()> {
        self.cleared.lock().unwrap().push(pane.to_owned());
        Ok(())
    }

    async fn kill_session(&self, name: &str) -> Result<()> {
        self.killed.lock().unwrap().push(name.to_owned());
        self.live.lock().unwrap().retain(|n| n != name);
        Ok(())
    }
}

/// Records the session settings written, so tests can assert the path and the
/// rendered JSON the server passed in.
#[derive(Default)]
struct FakeWorkspace {
    written: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl Workspace for FakeWorkspace {
    async fn write_session_settings(
        &self,
        settings_path: &str,
        settings_json: &str,
    ) -> Result<()> {
        self.written
            .lock()
            .unwrap()
            .push((settings_path.to_owned(), settings_json.to_owned()));
        Ok(())
    }
}

/// An in-memory transcript modelled as a list of file lines, keyed by path so
/// several sessions (each with its own transcript path) can be driven at once.
///
/// Each entry is one transcript line: `Some(msg)` is a parsed message,
/// `None` is a line that produces no message (blank / no-uuid / unparsable)
/// but still occupies a line and advances the cursor — exactly how the real
/// reader treats Claude Code's `file-history-snapshot` lines.
///
/// The default path matches the single-session [`submit`] helper, so the
/// single-session tests can keep pushing lines without naming a path.
const DEFAULT_TRANSCRIPT_PATH: &str = "/tmp/t.jsonl";

#[derive(Default)]
struct FakeTranscript {
    by_path: Mutex<HashMap<String, Vec<Option<TranscriptMessage>>>>,
    /// Paths the fake reports as absent from `exists`, modelling a transcript
    /// file that has been removed. By default every path is considered present,
    /// so the resume gate does not perturb the existing open/resume tests; a
    /// test marks a path missing via [`Self::mark_missing`] to exercise the
    /// resume-unavailable path.
    missing: Mutex<Vec<String>>,
}

#[async_trait]
impl Transcript for FakeTranscript {
    async fn read_from(&self, path: &str, from_line: usize) -> Result<TranscriptRead> {
        let by_path = self.by_path.lock().unwrap();
        let lines = by_path.get(path).cloned().unwrap_or_default();
        let messages = lines
            .iter()
            .enumerate()
            .skip(from_line)
            .filter_map(|(idx, line)| {
                line.clone().map(|mut msg| {
                    msg.seq = idx as i64;
                    msg
                })
            })
            .collect();
        Ok(TranscriptRead {
            messages,
            total_lines: lines.len(),
        })
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        Ok(!self.missing.lock().unwrap().iter().any(|p| p == path))
    }
}

#[derive(Default)]
struct FakeStoreInner {
    sessions: Vec<Session>,
    threads: Vec<Thread>,
    next_thread_id: i64,
    sends: Vec<PendingSend>,
    next_send_id: i64,
    messages: Vec<Message>,
    permissions: Vec<PermissionRequest>,
    next_perm_id: i64,
    transcript_lines_read: HashMap<SessionId, usize>,
}

#[derive(Default)]
struct FakeStore {
    inner: Mutex<FakeStoreInner>,
}

#[async_trait]
impl SessionStore for FakeStore {
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
        let mut g = self.inner.lock().unwrap();
        // Insert-if-absent, mirroring the real store's `INSERT OR IGNORE`: a
        // re-registration returns the existing session and its `main` thread.
        if let Some(session) = g.sessions.iter().find(|s| s.id == new.id).cloned() {
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
            transcript_path: new.transcript_path,
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

    async fn list_sessions(&self) -> Result<Vec<Session>> {
        Ok(self.inner.lock().unwrap().sessions.clone())
    }

    async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<Vec<SessionPageRow>> {
        let g = self.inner.lock().unwrap();
        // Build (session, last_activity_at) rows, then order exactly as the
        // SQL page query does: recency DESC, created_at DESC, id ASC, where
        // recency = last_activity_at or the session's created_at fallback.
        let mut rows: Vec<SessionPageRow> = g
            .sessions
            .iter()
            .map(|s| {
                let last_activity_at = g
                    .messages
                    .iter()
                    .filter(|m| m.session_id == s.id)
                    .map(|m| m.created_at.clone())
                    .max();
                (s.clone(), last_activity_at)
            })
            .collect();
        let recency = |row: &SessionPageRow| -> String {
            row.1.clone().unwrap_or_else(|| row.0.created_at.clone())
        };
        rows.sort_by(|a, b| {
            recency(b)
                .cmp(&recency(a))
                .then_with(|| b.0.created_at.cmp(&a.0.created_at))
                .then_with(|| a.0.id.as_str().cmp(b.0.id.as_str()))
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
                                && row.0.id.as_str() > c.id.as_str())))
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
            .map(|m| m.created_at.clone())
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

    async fn thread(&self, id: ThreadId) -> Result<Option<Thread>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .threads
            .iter()
            .find(|t| t.id == id)
            .cloned())
    }

    async fn list_threads(&self, session_id: &SessionId) -> Result<Vec<Thread>> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<Thread> = g
            .threads
            .iter()
            .filter(|t| &t.session_id == session_id)
            .cloned()
            .collect();
        out.sort_by_key(|t| t.id);
        Ok(out)
    }

    async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
        root_message_uuid: Option<&MessageUuid>,
    ) -> Result<Thread> {
        let mut g = self.inner.lock().unwrap();
        g.next_thread_id += 1;
        let thread = Thread {
            id: ThreadId(g.next_thread_id),
            session_id: session_id.clone(),
            title: title.to_owned(),
            parent_thread_id,
            root_message_uuid: root_message_uuid.cloned(),
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
    ) -> Result<PendingSend> {
        let mut g = self.inner.lock().unwrap();
        g.next_send_id += 1;
        let send = PendingSend {
            id: g.next_send_id,
            session_id: session_id.clone(),
            thread_id,
            semantic_parent_uuid: semantic_parent_uuid.cloned(),
            text: text.to_owned(),
            locator_quote: locator_quote.map(str::to_owned),
            status: PendingSendStatus::Pending,
            matched_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        g.sends.push(send.clone());
        Ok(send)
    }

    async fn head_pending_send(&self, session_id: &SessionId) -> Result<Option<PendingSend>> {
        let g = self.inner.lock().unwrap();
        Ok(g.sends
            .iter()
            .filter(|s| &s.session_id == session_id && s.status == PendingSendStatus::Pending)
            .min_by_key(|s| s.id)
            .cloned())
    }

    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g.sends.iter_mut().find(|s| s.id == id) {
            s.status = PendingSendStatus::Matched;
            s.matched_uuid = Some(matched_uuid.clone());
        }
        Ok(())
    }

    async fn match_pending_send(
        &self,
        session_id: &SessionId,
        trimmed_text: &str,
    ) -> Result<Option<PendingSend>> {
        let g = self.inner.lock().unwrap();
        Ok(g.sends
            .iter()
            .filter(|s| {
                &s.session_id == session_id
                    && s.status == PendingSendStatus::Pending
                    && s.text.trim() == trimmed_text
            })
            .min_by_key(|s| s.id)
            .cloned())
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
            s.status = PendingSendStatus::Cancelled;
        }
        Ok(())
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
    ) -> Result<PermissionRequest> {
        let mut g = self.inner.lock().unwrap();
        g.next_perm_id += 1;
        let req = PermissionRequest {
            id: g.next_perm_id,
            session_id: session_id.clone(),
            tool_name: tool_name.to_owned(),
            tool_input_json: tool_input_json.to_owned(),
            status: PermissionStatus::Pending,
            decision_reason: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            decided_at: None,
        };
        g.permissions.push(req.clone());
        Ok(req)
    }
}

fn user_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        uuid: MessageUuid::from(uuid),
        role: Role::User,
        linear_parent_uuid: None,
        prompt_id: None,
        content: vec![ContentBlock::Text { text: text.into() }],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        // The reader assigns the real line index on read; this is a placeholder.
        seq: 0,
    }
}

fn assistant_line(uuid: &str, text: &str) -> TranscriptMessage {
    TranscriptMessage {
        uuid: MessageUuid::from(uuid),
        role: Role::Assistant,
        linear_parent_uuid: None,
        prompt_id: None,
        content: vec![ContentBlock::Text { text: text.into() }],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        // The reader assigns the real line index on read; this is a placeholder.
        seq: 0,
    }
}

/// An assistant transcript line stamped with an explicit `created_at`, so a
/// test can give different sessions distinct last-activity timestamps.
fn assistant_line_at(uuid: &str, text: &str, created_at: &str) -> TranscriptMessage {
    TranscriptMessage {
        created_at: Some(created_at.into()),
        ..assistant_line(uuid, text)
    }
}

/// An assistant transcript line that issues a tool call (no author text).
fn tool_use_line(uuid: &str, id: &str, name: &str) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolUse {
            id: id.into(),
            name: name.into(),
            input: serde_json::Value::Null,
        }],
        ..assistant_line(uuid, "")
    }
}

/// A tool-result carrier line. Claude delivers these as `role: user` with no
/// author-written text; they belong to the in-flight turn, not a new human turn.
fn tool_result_line(uuid: &str, tool_use_id: &str) -> TranscriptMessage {
    TranscriptMessage {
        content: vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: serde_json::Value::Null,
            is_error: false,
        }],
        ..user_line(uuid, "")
    }
}

/// The base working directory the test interactor spawns sessions under.
const TEST_WORKDIR_BASE: &str = "/work";

/// The settings JSON the test interactor writes for each launch.
const TEST_SETTINGS_JSON: &str = r#"{"hooks":{}}"#;

/// The Delta-owned path the test interactor writes settings to and passes via
/// `claude --settings`. Outside any spawn workdir, on purpose.
const TEST_SETTINGS_PATH: &str = "/run/delta/settings.json";

fn interactor() -> Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace> {
    Interactor::new(
        FakeTmux::default(),
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        TEST_WORKDIR_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    )
}

/// An interactor whose tmux dispatch always fails.
fn interactor_with_failing_tmux() -> Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace>
{
    Interactor::new(
        FakeTmux {
            fail: true,
            ..Default::default()
        },
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        TEST_WORKDIR_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    )
}

fn submit(text: &str) -> UserPromptSubmitHook {
    UserPromptSubmitHook {
        prompt: text.into(),
        session_id: SessionId::from("sess-1"),
        transcript_path: "/tmp/t.jsonl".into(),
        cwd: "/work".into(),
    }
}

/// A submit hook for an explicit session id and transcript path, for the
/// multi-session routing tests.
fn submit_for(session_id: &str, transcript_path: &str, text: &str) -> UserPromptSubmitHook {
    UserPromptSubmitHook {
        prompt: text.into(),
        session_id: SessionId::from(session_id),
        transcript_path: transcript_path.into(),
        cwd: "/work".into(),
    }
}

#[tokio::test]
async fn ensure_session_spawns_a_session_in_its_own_workdir_when_absent() {
    let ix = interactor();

    let status = ix.ensure_session().await.unwrap();

    // A fresh cold start reports `Starting` and spawns a session in its own
    // per-token workdir under the base, with the settings written to Delta's own
    // path (not the workdir) and passed via `--settings`.
    assert_eq!(status, SessionLifecycle::Starting);
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session was spawned");
    assert_eq!(created[0].name, "delta-1", "named after the minted token");
    assert_eq!(created[0].workdir, "/work/delta-1", "<base>/<token>");
    // The launched argv pins the conversation's session id with the id Delta
    // minted and recorded on the pending spawn.
    let minted = ix.pending_session_ids().await.remove(0);
    assert_eq!(
        created[0].command,
        vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            TEST_SETTINGS_PATH.to_owned(),
            "--session-id".to_owned(),
            minted.as_str().to_owned(),
        ],
        "claude --settings <delta path> --session-id <minted id>"
    );
    let written = ix.workspace_fake().written.lock().unwrap().clone();
    assert_eq!(
        written,
        vec![(TEST_SETTINGS_PATH.to_owned(), TEST_SETTINGS_JSON.to_owned())],
        "settings go to Delta's path, not the spawn workdir"
    );
}

#[tokio::test]
async fn ensure_session_is_idempotent_while_a_spawn_is_live() {
    let ix = interactor();

    // First call spawns a session. It stays pending (no hook has bound it yet).
    ix.ensure_session().await.unwrap();
    // Second call finds a live (pending) spawn: reuse, no second spawn or write.
    let status = ix.ensure_session().await.unwrap();

    assert_eq!(status, SessionLifecycle::Ready);
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        1,
        "a live spawn must not be re-spawned"
    );
    assert_eq!(
        ix.workspace_fake().written.lock().unwrap().len(),
        1,
        "settings must not be rewritten when a spawn is already live"
    );
}

/// `ensure_session` is idempotent against a *bound* session too, not only a
/// pending spawn: once a hook has bound the spawn to a session id, a further
/// `ensure_session` reuses it (`Ready`) without spawning a second pane. This
/// pins the `bound` half of `has_any_live`, which the pending-only idempotency
/// test above does not exercise.
#[tokio::test]
async fn ensure_session_is_idempotent_while_a_session_is_bound() {
    let ix = interactor();

    // Spawn, then bind it via a hook carrying the spawn's minted session id.
    ix.ensure_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(
        ix.pane_for_session(&id).await.is_some(),
        "the spawn is now bound"
    );

    // A further ensure_session finds the bound session live: reuse, no re-spawn.
    let status = ix.ensure_session().await.unwrap();
    assert_eq!(status, SessionLifecycle::Ready);
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        1,
        "a bound session must not be re-spawned"
    );
}

#[tokio::test]
async fn first_submit_registers_session() {
    let ix = interactor();
    let (events, _) = ix.on_user_prompt_submit(submit("hi")).await.unwrap();
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-1"),
    }));
    assert!(ix
        .store()
        .session(&SessionId::from("sess-1"))
        .await
        .unwrap()
        .is_some());
}

/// Each distinct session id registers independently on its first
/// `UserPromptSubmit`. A second submit for an already-registered id does not
/// re-register, but a submit for a new id does — registration is "first contact
/// for THIS id", not "first ever".
#[tokio::test]
async fn submit_registers_each_session_id_independently() {
    let ix = interactor();

    // First contact for sess-1 registers it.
    let (events1, _) = ix
        .on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "hi"))
        .await
        .unwrap();
    assert!(events1.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-1"),
    }));

    // A second submit for sess-1 must NOT re-register it.
    let (events1b, _) = ix
        .on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "again"))
        .await
        .unwrap();
    assert!(
        !events1b
            .iter()
            .any(|e| matches!(e, SessionEvent::SessionRegistered { .. })),
        "an already-registered id must not re-register"
    );

    // First contact for a DIFFERENT id registers that one too.
    let (events2, _) = ix
        .on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "hi"))
        .await
        .unwrap();
    assert!(events2.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-2"),
    }));

    // Both sessions now exist.
    let ids: Vec<String> = ix
        .store()
        .list_sessions()
        .await
        .unwrap()
        .iter()
        .map(|s| s.id.as_str().to_owned())
        .collect();
    assert_eq!(ids, vec!["sess-1".to_owned(), "sess-2".to_owned()]);
}

/// `on_stop` routes by the hook's own session id: a `Stop` for one session syncs
/// only that session's transcript, leaving the other session untouched.
#[tokio::test]
async fn on_stop_routes_sync_by_hook_session_id() {
    let ix = interactor();

    // Register two sessions, each with its own transcript path.
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "seed"))
        .await
        .unwrap();

    // Each session's transcript grows by one assistant line, on its own path.
    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-1", "reply one"));
    ix.transcript_fake()
        .push_to("/tmp/s2.jsonl", assistant_line("a-2", "reply two"));

    // A `Stop` for sess-1 must ingest only sess-1's line.
    ix.on_stop(crate::ports::StopHook {
        session_id: SessionId::from("sess-1"),
        stop_reason: None,
    })
    .await
    .unwrap();

    assert_eq!(
        ix.store()
            .message_count(&SessionId::from("sess-1"))
            .await
            .unwrap(),
        1,
        "the Stop for sess-1 ingested its assistant line"
    );
    assert_eq!(
        ix.store()
            .message_count(&SessionId::from("sess-2"))
            .await
            .unwrap(),
        0,
        "sess-2 was not synced by a Stop addressed to sess-1"
    );
}

/// `poll_transcript` syncs every registered session and groups the new lines per
/// session, so the caller can announce each session's growth separately.
#[tokio::test]
async fn poll_transcript_groups_new_lines_per_session() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "seed"))
        .await
        .unwrap();

    // Both sessions flush a late assistant line.
    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-1", "reply one"));
    ix.transcript_fake()
        .push_to("/tmp/s2.jsonl", assistant_line("a-2", "reply two"));

    let groups = ix.poll_transcript().await.unwrap();
    assert_eq!(groups.len(), 2, "one group per session that grew");

    // Each group carries exactly its own session's new line.
    let mut by_session: Vec<(String, Vec<String>)> = groups
        .iter()
        .map(|g| {
            (
                g[0].session_id.as_str().to_owned(),
                g.iter().map(|m| m.uuid.as_str().to_owned()).collect(),
            )
        })
        .collect();
    by_session.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        by_session,
        vec![
            ("sess-1".to_owned(), vec!["a-1".to_owned()]),
            ("sess-2".to_owned(), vec!["a-2".to_owned()]),
        ]
    );

    // A second poll with no new lines yields no groups (per-session cursors
    // advanced independently).
    assert!(ix.poll_transcript().await.unwrap().is_empty());
}

/// `list_sessions` lists every registered session, each annotated with its
/// live (open) state and `main` thread id; `threads_for` scopes the thread
/// tree to a single session by id. With no messages, both sessions share the
/// same recency fallback (`created_at`), so the `id` tiebreaker decides their
/// order; recency ordering proper is covered by
/// [`list_sessions_orders_by_most_recent_activity`].
#[tokio::test]
async fn list_sessions_annotates_each_with_open_state_and_threads_route_by_id() {
    let ix = interactor();

    // No session yet: the list is empty.
    assert!(ix.list_sessions().await.unwrap().is_empty());

    // Register two sessions in order. Their hooks arrive in a cwd with no
    // matching pending spawn, so they register as external, closed data sessions
    // (no live pane).
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "seed"))
        .await
        .unwrap();

    let listings = ix.list_sessions().await.unwrap();
    let ids: Vec<_> = listings
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect();
    assert_eq!(
        ids,
        vec!["sess-1", "sess-2"],
        "equal recency falls back to the deterministic id tiebreaker"
    );
    assert!(
        listings.iter().all(|l| !l.open),
        "externally-registered sessions are closed (no live pane)"
    );
    assert!(
        listings.iter().all(|l| l.main_thread_id.value() > 0),
        "every listing carries its main thread id"
    );
    assert!(
        listings.iter().all(|l| l.last_activity_at.is_none()),
        "sessions with no ingested messages have no last activity"
    );

    // `threads_for` is scoped to the named session: only its own threads.
    let threads = ix.threads_for(&SessionId::from("sess-2")).await.unwrap();
    assert!(
        !threads.is_empty() && threads.iter().all(|t| t.session_id.as_str() == "sess-2"),
        "threads belong to the requested session only"
    );

    // An unknown session id is a clean SessionNotFound, not an empty list.
    let err = ix
        .threads_for(&SessionId::from("nope"))
        .await
        .expect_err("unknown session id is rejected");
    assert!(matches!(err, crate::error::Error::SessionNotFound(_)));
}

/// The navigator lists sessions most-recently-active first. The sort key is a
/// session's last activity (`MAX(message.created_at)`), falling back to its own
/// `created_at` when it has no messages — so a message-less session sorts above
/// one whose only activity is older than that fallback.
#[tokio::test]
async fn list_sessions_orders_by_most_recent_activity() {
    let ix = interactor();

    // Three sessions registered in id order; all share the same `created_at`
    // (the fake store stamps a fixed registration time).
    ix.on_user_prompt_submit(submit_for("sess-old", "/tmp/old.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-new", "/tmp/new.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-quiet", "/tmp/quiet.jsonl", "seed"))
        .await
        .unwrap();

    // `sess-old` last spoke before the shared registration time; `sess-new`
    // spoke after it. `sess-quiet` has no messages, so it falls back to its
    // `created_at` (the shared registration time, "2026-01-01T00:00:00Z").
    ix.transcript_fake().push_to(
        "/tmp/old.jsonl",
        assistant_line_at("a-old", "older", "2025-12-31T00:00:00Z"),
    );
    ix.transcript_fake().push_to(
        "/tmp/new.jsonl",
        assistant_line_at("a-new", "newer", "2026-02-01T00:00:00Z"),
    );
    ix.poll_transcript().await.unwrap();

    let ids: Vec<_> = ix
        .list_sessions()
        .await
        .unwrap()
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect();
    assert_eq!(
        ids,
        vec!["sess-new", "sess-quiet", "sess-old"],
        "most recent activity first; a message-less session sorts on its \
         created_at fallback, above one whose only activity is older"
    );
}

/// Equal recency keys order deterministically by the `created_at` then `id`
/// tiebreaker, so the list never reshuffles between calls for sessions with the
/// same last-activity timestamp.
#[tokio::test]
async fn list_sessions_breaks_recency_ties_deterministically() {
    let ix = interactor();

    // Two sessions, no messages: both fall back to the same (shared)
    // `created_at`, so only the `id` tiebreaker distinguishes them.
    ix.on_user_prompt_submit(submit_for("sess-b", "/tmp/b.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-a", "/tmp/a.jsonl", "seed"))
        .await
        .unwrap();

    let order = || async {
        ix.list_sessions()
            .await
            .unwrap()
            .iter()
            .map(|l| l.session.id.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    // Registered b-then-a, but the ascending `id` tiebreaker puts "sess-a"
    // first, and repeated calls agree.
    assert_eq!(order().await, vec!["sess-a", "sess-b"]);
    assert_eq!(order().await, vec!["sess-a", "sess-b"], "order is stable");
}

/// Paging across two pages reproduces the single-shot recency order of
/// `list_sessions_orders_by_most_recent_activity`: most recent first, with a
/// message-less session falling back to its `created_at`.
#[tokio::test]
async fn list_sessions_page_reproduces_recency_order_across_pages() {
    let ix = interactor();

    ix.on_user_prompt_submit(submit_for("sess-old", "/tmp/old.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-new", "/tmp/new.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-quiet", "/tmp/quiet.jsonl", "seed"))
        .await
        .unwrap();
    ix.transcript_fake().push_to(
        "/tmp/old.jsonl",
        assistant_line_at("a-old", "older", "2025-12-31T00:00:00Z"),
    );
    ix.transcript_fake().push_to(
        "/tmp/new.jsonl",
        assistant_line_at("a-new", "newer", "2026-02-01T00:00:00Z"),
    );
    ix.poll_transcript().await.unwrap();

    // Page through two at a time; concatenating the pages yields the same order
    // the all-at-once method asserts.
    let first = ix.list_sessions_page(None, 2).await.unwrap();
    let first_ids: Vec<_> = first
        .listings
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect();
    assert_eq!(first_ids, vec!["sess-new", "sess-quiet"]);
    assert!(first.next.is_some(), "a full page yields a cursor");

    let second = ix.list_sessions_page(first.next, 2).await.unwrap();
    let second_ids: Vec<_> = second
        .listings
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect();
    assert_eq!(second_ids, vec!["sess-old"]);
    assert!(
        second.next.is_none(),
        "a short last page yields no further cursor"
    );
}

/// Equal-recency sessions page in the same deterministic id-ascending order as
/// `list_sessions_breaks_recency_ties_deterministically`, with the cursor
/// stepping cleanly across the tie group.
#[tokio::test]
async fn list_sessions_page_breaks_recency_ties_deterministically() {
    let ix = interactor();

    // Two message-less sessions share the same created_at fallback, so only the
    // ascending id tiebreaker orders them.
    ix.on_user_prompt_submit(submit_for("sess-b", "/tmp/b.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-a", "/tmp/a.jsonl", "seed"))
        .await
        .unwrap();

    let first = ix.list_sessions_page(None, 1).await.unwrap();
    assert_eq!(first.listings[0].session.id.as_str(), "sess-a");

    let second = ix.list_sessions_page(first.next, 1).await.unwrap();
    assert_eq!(second.listings[0].session.id.as_str(), "sess-b");
}

/// Page rows carry the same `open` and `main_thread_id` enrichment the
/// all-at-once method does: a bound session pages as `open: true` with its
/// trunk thread id; the inline `last_activity_at` is preserved.
#[tokio::test]
async fn list_sessions_page_annotates_open_state_and_threads() {
    let ix = interactor();

    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(ix.pane_for_session(&id).await.is_some(), "bound = open");

    let page = ix.list_sessions_page(None, 30).await.unwrap();
    let listing = page
        .listings
        .iter()
        .find(|l| l.session.id == id)
        .expect("the session is paged");
    assert!(listing.open, "a bound session pages as open");
    assert!(
        listing.main_thread_id.value() > 0,
        "the page carries the trunk thread id"
    );
    assert!(
        listing.last_activity_at.is_none(),
        "no ingested messages means no inline last activity"
    );
}

/// The `open` flag tracks live state: a session with a bound pane lists as
/// `open: true`, and once closed it lists as `open: false` while still present.
/// The annotated-as-closed test above only pins the closed side, so this pins
/// the open side (and the open→closed transition) that the API surfaces.
#[tokio::test]
async fn list_sessions_marks_a_bound_session_open_and_a_closed_one_not() {
    let ix = interactor();

    // Spawn and bind a session: it now has a live pane.
    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(ix.pane_for_session(&id).await.is_some(), "bound = open");

    let open_state = |listings: &[crate::SessionListing]| {
        listings
            .iter()
            .find(|l| l.session.id == id)
            .map(|l| l.open)
            .expect("the session is listed")
    };

    assert!(
        open_state(&ix.list_sessions().await.unwrap()),
        "a bound session lists as open"
    );

    // Closing tears the pane down but keeps the row: it now lists as closed.
    ix.close_session(&id).await.unwrap();
    assert!(
        !open_state(&ix.list_sessions().await.unwrap()),
        "a closed session still lists, now as not open"
    );
}

#[tokio::test]
async fn fifo_head_matches_and_marks_send() {
    let ix = interactor();
    // Register and obtain main thread.
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let main = ix
        .store()
        .main_thread_id(&SessionId::from("sess-1"))
        .await
        .unwrap();

    // Queue a send (also dispatches to fake tmux).
    let pending = ix
        .enqueue_send(to(main), "hello world", Some("[quote]"))
        .await
        .unwrap();
    assert_eq!(pending.status, PendingSendStatus::Pending);

    // The transcript now contains the matching user line.
    ix.transcript_fake()
        .push(user_line("uuid-1", "hello world"));

    let (events, additional) = ix
        .on_user_prompt_submit(submit("hello world"))
        .await
        .unwrap();
    // A locator quote → first entry into a branch: the locator frame plus a
    // note binding the quote to the thread the send targets.
    let expected =
        super::frame_branch_entry_context(&super::frame_locator_context("[quote]").unwrap(), main);
    assert_eq!(additional, Some(expected));
    let started = events
        .iter()
        .find_map(|e| match e {
            SessionEvent::TurnStarted {
                matched_uuid,
                pending_send_id,
                ..
            } => Some((matched_uuid.clone(), *pending_send_id)),
            _ => None,
        })
        .expect("turn started event");
    assert_eq!(started.0, MessageUuid::from("uuid-1"));
    assert_eq!(started.1, pending.id);

    // Marked matched; no longer the head.
    let head = ix
        .store()
        .head_pending_send(&SessionId::from("sess-1"))
        .await
        .unwrap();
    assert!(head.is_none());
}

#[tokio::test]
async fn unmatched_prompt_is_external_input() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    ix.transcript_fake()
        .push(user_line("u-ext", "typed directly"));

    let (events, additional) = ix
        .on_user_prompt_submit(submit("typed directly"))
        .await
        .unwrap();
    assert!(additional.is_none());
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::ExternalInput { prompt, .. } if prompt == "typed directly"
    )));
}

/// Drive a full send round-trip: queue the send, push its matching user line,
/// and run the `UserPromptSubmit` hook so the line is attributed to the send's
/// thread (making it the session's `latest_user_thread`). Returns the injected
/// `additionalContext` for that send so a caller can assert on it.
async fn round_trip(
    ix: &Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace>,
    target: SendTarget,
    text: &str,
    quote: Option<&str>,
    uuid: &str,
) -> (PendingSend, Option<String>) {
    let pending = ix.enqueue_send(target, text, quote).await.unwrap();
    ix.transcript_fake().push(user_line(uuid, text));
    let (_events, additional) = ix.on_user_prompt_submit(submit(text)).await.unwrap();
    (pending, additional)
}

#[tokio::test]
async fn revisit_to_branch_injects_switch_note_with_root_quote() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // First entry into a branch off some message: this creates the child thread
    // (titled after the locator quote) and makes it the latest user thread.
    let parent = MessageUuid::from("uuid-parent");
    let (branch_send, _) = round_trip(
        &ix,
        branch_off(main, &parent),
        "into branch",
        Some("[root quote]"),
        "u-branch",
    )
    .await;
    let child = branch_send.thread_id;
    assert_ne!(child, main);

    // Move back to main (no quote): the latest user thread becomes main again.
    round_trip(&ix, to(main), "back on main", None, "u-main").await;

    // Now re-visit the child thread (no quote): a thread switch from main to the
    // child, so the note re-cites the child's root quote and re-focuses the
    // model onto that earlier thread.
    let (_, additional) = round_trip(&ix, to(child), "more on branch", None, "u-revisit").await;
    let expected = super::frame_thread_switch_context(main, child, Some("[root quote]"));
    assert_eq!(additional, Some(expected));
    let note = additional.unwrap();
    assert!(note.contains(&format!("thread:{}", child.value())));
    assert!(note.contains("[root quote]"));
    assert!(note.contains("not replying to the message immediately above"));
}

#[tokio::test]
async fn revisit_to_main_injects_switch_note_without_quote() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Enter a branch first, so the latest user thread is the child.
    let parent = MessageUuid::from("uuid-parent");
    let (branch_send, _) = round_trip(
        &ix,
        branch_off(main, &parent),
        "into branch",
        Some("[root quote]"),
        "u-branch",
    )
    .await;
    let child = branch_send.thread_id;

    // Return to main (no quote): a switch from the child back to the trunk.
    // `main` has no root passage, so the note names it without citing a quote.
    let (_, additional) = round_trip(&ix, to(main), "back to main", None, "u-main").await;
    let expected = super::frame_thread_switch_context(child, main, None);
    assert_eq!(additional, Some(expected));
    let note = additional.unwrap();
    assert!(note.contains("the main thread"));
    assert!(!note.contains('"'), "no quote is cited for main");
    assert!(note.contains("not replying to the message immediately above"));
}

#[tokio::test]
async fn unknown_previous_thread_injects_nothing() {
    // Regression: on the first prompt after a session resume (and on the very
    // first turn), no user line is persisted yet, so `latest_user_thread`
    // reports `None` at the moment `thread_switch_context` runs. That is an
    // UNKNOWN previous thread, not a switch — the user may simply be continuing.
    // Asserting a switch there ("The user has switched to thread:N") is false
    // and misleads the model, so nothing must be injected.
    let ix = interactor();
    // Register the session (creates its `main` thread) without persisting any
    // user line: `submit` carries no matching transcript line, so it syncs
    // nothing. This mirrors the resume boundary where no user line is visible
    // to `latest_user_thread` yet.
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // No prior persisted user line exists, so `latest_user_thread` is `None`.
    assert!(
        ix.store()
            .latest_user_thread(&session)
            .await
            .unwrap()
            .is_none(),
        "precondition: previous thread is unknown"
    );

    // A plain send to main with no locator quote: the previous thread is
    // unknown, so this is not a switch and no re-focus note is injected.
    let (_, additional) = round_trip(&ix, to(main), "first prompt", None, "u-1").await;
    assert!(
        additional.is_none(),
        "unknown previous thread must not inject a switch note, got: {additional:?}"
    );
}

#[tokio::test]
async fn same_thread_continuation_injects_nothing() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Two consecutive plain sends to the same (main) thread. The second is a
    // same-thread continuation, so nothing is injected.
    round_trip(&ix, to(main), "first on main", None, "u-1").await;
    let (_, additional) = round_trip(&ix, to(main), "second on main", None, "u-2").await;
    assert!(additional.is_none());
}

#[tokio::test]
async fn branch_send_creates_child_thread() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let main = ix
        .store()
        .main_thread_id(&SessionId::from("sess-1"))
        .await
        .unwrap();

    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();

    assert_ne!(pending.thread_id, main, "branch send targets a new thread");
    assert_eq!(pending.semantic_parent_uuid, Some(parent.clone()));
    let child = ix.store().thread(pending.thread_id).await.unwrap().unwrap();
    assert_eq!(child.parent_thread_id, Some(main));
    assert_eq!(child.root_message_uuid, Some(parent));
}

#[tokio::test]
async fn plain_send_attributes_user_and_assistant_to_main() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let pending = ix
        .enqueue_send(to(main), "hello world", None)
        .await
        .unwrap();
    assert_eq!(pending.thread_id, main);

    // The matching user line is ingested + correlated.
    ix.transcript_fake().push(user_line("u-1", "hello world"));
    ix.on_user_prompt_submit(submit("hello world"))
        .await
        .unwrap();

    // The assistant response arrives and is ingested at Stop.
    ix.transcript_fake().push(assistant_line("a-1", "hi there"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    let view = ix.thread_view(main).await.unwrap();
    let uuids: Vec<&str> = view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(uuids.contains(&"u-1"), "user message lands on main");
    assert!(uuids.contains(&"a-1"), "assistant message lands on main");
}

#[tokio::test]
async fn branch_send_attributes_user_and_assistant_to_child() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Branch off some existing message and queue the first branch send.
    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);

    // The matching user line is present at submit time, so it is matched to the
    // pending send and attributed to the child during this sync.
    ix.transcript_fake().push(user_line("u-b", "branch text"));
    ix.on_user_prompt_submit(submit("branch text"))
        .await
        .unwrap();

    // The assistant response is ingested at Stop and must carry forward to the
    // child thread (the thread of the latest user message).
    ix.transcript_fake()
        .push(assistant_line("a-b", "branch reply"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // Both land on the child, not main.
    let child_view = ix.thread_view(child).await.unwrap();
    let child_uuids: Vec<&str> = child_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(child_uuids.contains(&"u-b"), "user message lands on child");
    assert!(
        child_uuids.contains(&"a-b"),
        "assistant message lands on child"
    );

    // The matched user message also carries the branch semantic parent.
    let user_msg = child_view
        .iter()
        .find(|m| m.uuid.as_str() == "u-b")
        .unwrap();
    assert_eq!(user_msg.semantic_parent_uuid, Some(parent));

    // And neither leaked onto main.
    let main_view = ix.thread_view(main).await.unwrap();
    let main_uuids: Vec<&str> = main_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(!main_uuids.contains(&"u-b"));
    assert!(!main_uuids.contains(&"a-b"));
}

/// A tool call mid-turn on a branch must not reset attribution to `main`. Claude
/// writes the `tool_result` as a `role: user` line; treating it as a new human
/// turn used to drop the result and the assistant's continuation onto `main`, so
/// the branch lost the turn's tail (its last message). Regression test.
#[tokio::test]
async fn tool_result_mid_branch_turn_stays_on_the_branch() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Start a branch turn and match its user line onto the child thread.
    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);
    ix.transcript_fake().push(user_line("u-b", "branch text"));
    ix.on_user_prompt_submit(submit("branch text"))
        .await
        .unwrap();

    // The turn calls a tool: assistant tool_use, the tool_result (a `role: user`
    // line), then the assistant's final text — all ingested together at Stop.
    ix.transcript_fake().push(tool_use_line("a-call", "t1", "Bash"));
    ix.transcript_fake().push(tool_result_line("u-res", "t1"));
    ix.transcript_fake()
        .push(assistant_line("a-final", "after the tool"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // The whole tail stays on the branch.
    let child_uuids: Vec<String> = ix
        .thread_view(child)
        .await
        .unwrap()
        .iter()
        .map(|m| m.uuid.as_str().to_owned())
        .collect();
    assert!(child_uuids.contains(&"a-call".to_owned()));
    assert!(
        child_uuids.contains(&"u-res".to_owned()),
        "tool_result stays on the branch turn, not main"
    );
    assert!(
        child_uuids.contains(&"a-final".to_owned()),
        "the assistant continuation after the tool stays on the branch"
    );

    // Nothing leaked onto main.
    let main_uuids: Vec<String> = ix
        .thread_view(main)
        .await
        .unwrap()
        .iter()
        .map(|m| m.uuid.as_str().to_owned())
        .collect();
    assert!(!main_uuids.contains(&"u-res".to_owned()));
    assert!(!main_uuids.contains(&"a-final".to_owned()));
}

/// Reproduces the thread-attribution timing bug: the `UserPromptSubmit` hook
/// fires before the user line is written to the JSONL, so nothing is attributed
/// in that sync. Both the user line and the assistant reply arrive together in a
/// later sync (as happens at `Stop`) and must still land on the branch thread.
#[tokio::test]
async fn branch_send_attributes_late_arriving_lines_to_child() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Queue a branch send. The user line is NOT in the transcript yet.
    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(
            branch_off(main, &parent),
            "branch text",
            Some("quoted line"),
        )
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);

    // The hook fires before the user line is flushed to the JSONL. The locator
    // quote frame (plus the branch-entry note) is still returned (text-based),
    // but nothing is attributed yet.
    let (events, additional) = ix
        .on_user_prompt_submit(submit("branch text"))
        .await
        .unwrap();
    let expected = super::frame_branch_entry_context(
        &super::frame_locator_context("quoted line").unwrap(),
        child,
    );
    assert_eq!(additional, Some(expected));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::TurnStarted { .. })),
        "no turn started while the user line is absent"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "a queued send matched, so this is not external input"
    );
    // Still pending: nothing was matched yet.
    let head = ix.store().head_pending_send(&session).await.unwrap();
    assert_eq!(head.map(|p| p.id), Some(pending.id));

    // Later (at Stop) BOTH the user line and the assistant reply arrive in one
    // sync. Attribution must key off the pending send, not the hook timing.
    ix.transcript_fake().push(user_line("u-b", "branch text"));
    ix.transcript_fake()
        .push(assistant_line("a-b", "branch reply"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // Both messages land on the child thread.
    let child_view = ix.thread_view(child).await.unwrap();
    let child_uuids: Vec<&str> = child_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(child_uuids.contains(&"u-b"), "user message lands on child");
    assert!(
        child_uuids.contains(&"a-b"),
        "assistant message lands on child"
    );

    // The user message carries the branch semantic parent.
    let user_msg = child_view
        .iter()
        .find(|m| m.uuid.as_str() == "u-b")
        .unwrap();
    assert_eq!(user_msg.semantic_parent_uuid, Some(parent));

    // The pending send is now matched (to the user line uuid).
    let send = ix
        .store()
        .inner
        .lock()
        .unwrap()
        .sends
        .iter()
        .find(|s| s.id == pending.id)
        .cloned()
        .unwrap();
    assert_eq!(send.status, PendingSendStatus::Matched);
    assert_eq!(send.matched_uuid, Some(MessageUuid::from("u-b")));

    // Neither leaked onto main.
    let main_view = ix.thread_view(main).await.unwrap();
    let main_uuids: Vec<&str> = main_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(!main_uuids.contains(&"u-b"));
    assert!(!main_uuids.contains(&"a-b"));
}

/// A branch send creates the child thread with a provisional title derived from
/// the locator quote, instead of the placeholder "untitled".
#[tokio::test]
async fn branch_send_titles_child_from_locator_quote() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let main = ix
        .store()
        .main_thread_id(&SessionId::from("sess-1"))
        .await
        .unwrap();

    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(
            branch_off(main, &parent),
            "branch text",
            Some("  the quoted source line  "),
        )
        .await
        .unwrap();
    let child = ix.store().thread(pending.thread_id).await.unwrap().unwrap();
    assert_eq!(child.title, "the quoted source line");

    // With no quote, the title falls back to "untitled".
    let pending2 = ix
        .enqueue_send(branch_off(main, &parent), "branch text 2", None)
        .await
        .unwrap();
    let child2 = ix
        .store()
        .thread(pending2.thread_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(child2.title, "untitled");
}

#[tokio::test]
async fn enqueue_send_to_unknown_thread_is_thread_not_found() {
    use crate::error::Error;

    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();

    // A thread id that was never created (stale/wrong id from the browser).
    let err = ix
        .enqueue_send(to(ThreadId(999)), "hello", None)
        .await
        .expect_err("unknown thread must be rejected");
    assert!(matches!(err, Error::ThreadNotFound(999)));
}

#[tokio::test]
async fn failed_dispatch_rolls_back_pending_send_and_returns_error() {
    use crate::error::Error;

    let ix = interactor_with_failing_tmux();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // The dispatch fails, so the use case must surface the tmux error...
    let err = ix
        .enqueue_send(to(main), "never delivered", None)
        .await
        .expect_err("a failed dispatch must propagate the error");
    assert!(matches!(err, Error::Tmux(_)));

    // ...and the just-written row must not block the FIFO head: it was rolled
    // back to `cancelled`, so the head is clear for future correlation.
    let head = ix.store().head_pending_send(&session).await.unwrap();
    assert!(
        head.is_none(),
        "the cancelled row must not remain the FIFO head"
    );
}

#[tokio::test]
async fn pre_tool_use_records_request_and_notifies() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let events = ix
        .on_pre_tool_use(&SessionId::from("sess-1"), "Bash", r#"{"command":"ls"}"#)
        .await
        .unwrap();
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::PermissionRequested { tool_name, .. } if tool_name == "Bash"
    )));
}

/// Regression test for the line-vs-message offset stall.
///
/// A no-uuid line (Claude Code's `file-history-snapshot`) trails turn 1. With
/// the old message-count offset, the cursor (a message count) lagged behind the
/// file line count by one for every skipped line, so the next sync re-read
/// already-ingested lines, `seq` drifted, and the latest turn stopped being
/// ingested. With the line-based cursor, the skipped line still advances the
/// cursor, so turn 2 is ingested cleanly on the second sync.
#[tokio::test]
async fn skipped_line_does_not_stall_later_turn_ingestion() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // --- Sync 1: turn 1 (user + assistant) followed by a no-uuid line. ---
    ix.enqueue_send(to(main), "turn one", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-1", "turn one")); // line 0
    ix.transcript_fake()
        .push(assistant_line("a-1", "reply one")); // line 1
    ix.transcript_fake().push_skipped_line(); // line 2: file-history-snapshot
    ix.on_user_prompt_submit(submit("turn one")).await.unwrap();
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // Turn 1 is ingested and the cursor advanced past the trailing no-uuid line
    // (3 lines), not merely past the 2 messages.
    let after1 = ix.thread_view(main).await.unwrap();
    let uuids1: Vec<&str> = after1.iter().map(|m| m.uuid.as_str()).collect();
    assert!(uuids1.contains(&"u-1"));
    assert!(uuids1.contains(&"a-1"));
    assert_eq!(
        ix.store().transcript_lines_read(&session).await.unwrap(),
        3,
        "the cursor counts the no-uuid line, not just the messages"
    );

    // --- Sync 2: turn 2 appended. Previously this stalled. ---
    ix.enqueue_send(to(main), "turn two", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-2", "turn two")); // line 3
    ix.transcript_fake()
        .push(assistant_line("a-2", "reply two")); // line 4
    ix.on_user_prompt_submit(submit("turn two")).await.unwrap();
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    let view = ix.thread_view(main).await.unwrap();
    let uuids: Vec<&str> = view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(uuids.contains(&"u-2"), "turn 2 user is ingested");
    assert!(uuids.contains(&"a-2"), "turn 2 assistant is ingested");

    // seq follows the true file line order (line indices), monotonic and gapless
    // across the skipped line, with no duplicates.
    let by_uuid = |u: &str| view.iter().find(|m| m.uuid.as_str() == u).unwrap().seq;
    assert_eq!(by_uuid("u-1"), 0);
    assert_eq!(by_uuid("a-1"), 1);
    assert_eq!(by_uuid("u-2"), 3);
    assert_eq!(by_uuid("a-2"), 4);
    assert_eq!(view.len(), 4, "no duplicates from re-reading lines");
    // thread_view orders by seq; assert it is strictly increasing (monotonic,
    // no duplicate line indices).
    let seqs: Vec<i64> = view.iter().map(|m| m.seq).collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "seq is strictly increasing in line order: {seqs:?}"
    );
}

/// Reproduces the core "responses don't appear" bug: Claude Code flushes the
/// final assistant line to the JSONL *after* the `Stop` hook fires, so the
/// hook's sync misses it. Only a later `poll_transcript` (the continuous tail)
/// ingests it and returns it.
#[tokio::test]
async fn poll_transcript_ingests_assistant_line_flushed_after_stop() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Queue a send and run the user-turn hooks. At `Stop` only the user line is
    // present — the assistant reply has not been flushed yet.
    ix.enqueue_send(to(main), "hello world", None)
        .await
        .unwrap();
    ix.transcript_fake().push(user_line("u-1", "hello world"));
    ix.on_user_prompt_submit(submit("hello world"))
        .await
        .unwrap();
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // The user line is ingested, but the assistant reply is absent: the gap the
    // hook-only ingestion leaves.
    let after_stop = ix.thread_view(main).await.unwrap();
    let uuids_after_stop: Vec<&str> = after_stop.iter().map(|m| m.uuid.as_str()).collect();
    assert!(
        uuids_after_stop.contains(&"u-1"),
        "user line ingested at hooks"
    );
    assert!(
        !uuids_after_stop.contains(&"a-1"),
        "assistant reply is not ingested by the hooks (the bug)"
    );

    // Claude Code now flushes the assistant line. A poll (no hook) catches it.
    // The single session yields one group carrying just the new line.
    ix.transcript_fake().push(assistant_line("a-1", "hi there"));
    let polled = ix.poll_transcript().await.unwrap();
    assert_eq!(
        polled.len(),
        1,
        "one group for the single registered session"
    );
    let polled_uuids: Vec<&str> = polled[0].iter().map(|m| m.uuid.as_str()).collect();
    assert_eq!(polled_uuids, vec!["a-1"], "poll returns only the new line");
    assert_eq!(
        polled[0][0].thread_id, main,
        "the assistant reply is attributed to the turn's thread"
    );

    // It is now persisted on the thread.
    let final_view = ix.thread_view(main).await.unwrap();
    let final_uuids: Vec<&str> = final_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(
        final_uuids.contains(&"a-1"),
        "poll ingested the assistant reply"
    );

    // A second poll with no new lines returns nothing (cursor advanced).
    let again = ix.poll_transcript().await.unwrap();
    assert!(again.is_empty(), "no new lines, nothing returned");
}

/// `poll_transcript` is a no-op before any session is registered.
#[tokio::test]
async fn poll_transcript_without_session_is_empty() {
    let ix = interactor();
    let polled = ix.poll_transcript().await.unwrap();
    assert!(polled.is_empty());
}

/// A submit hook for an explicit cwd. The cwd no longer drives spawn binding
/// (that is keyed by the Delta-minted session id), so this is used both for
/// external-claude registration tests and for binding tests that pass a spawn's
/// minted session id while exercising an arbitrary cwd.
fn submit_in(
    session_id: &str,
    transcript_path: &str,
    cwd: &str,
    text: &str,
) -> UserPromptSubmitHook {
    UserPromptSubmitHook {
        prompt: text.into(),
        session_id: SessionId::from(session_id),
        transcript_path: transcript_path.into(),
        cwd: cwd.into(),
    }
}

/// Composer-first send with no prior session: it spawns a fresh session,
/// defers the first prompt, and once a `UserPromptSubmit` binds the spawn the
/// deferred `pending_send` is written and the first user line correlates (the
/// turn starts) through the normal machinery.
#[tokio::test]
async fn composer_first_send_defers_first_prompt_until_bind() {
    let ix = interactor();

    // No session exists yet. The send spawns a fresh session and returns a
    // synthetic (not-yet-persisted) pending row.
    let returned = ix
        .enqueue_send(SendTarget::NewSession, "first message", None)
        .await
        .unwrap();
    assert_eq!(returned.id, 0, "no row persisted before the spawn binds");
    assert_eq!(returned.text, "first message");

    // The spawn created exactly one tmux session in its own workdir, and no
    // pending_send row was written yet (the session id does not exist).
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].name, "delta-1");
    assert_eq!(created[0].workdir, "/work/delta-1");

    // The deferred first prompt was actually typed into the spawned pane up
    // front (otherwise Claude would sit idle and never fire the hook that binds
    // the spawn).
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(
        sent,
        vec![("delta-1:0.0".to_owned(), "first message".to_owned())],
        "the first prompt is dispatched into the fresh pane"
    );

    // Delta pinned the conversation's session id at spawn time; read it back so
    // the hook can carry the exact id (a real hook reports the pinned id).
    let session_id = ix.pending_session_ids().await.remove(0);

    // The first UserPromptSubmit reports that pinned session id. It binds the
    // spawn to the now-known session id, registers the session, and writes the
    // deferred pending_send BEFORE attribution — so the user line correlates.
    ix.transcript_fake()
        .push_to("/work/delta-1/t.jsonl", user_line("u-1", "first message"));
    let (events, _) = ix
        .on_user_prompt_submit(submit_in(
            session_id.as_str(),
            "/work/delta-1/t.jsonl",
            "/work/delta-1",
            "first message",
        ))
        .await
        .unwrap();

    // The session registered and the first turn started (the deferred send was
    // written and matched the user line).
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: session_id.clone(),
    }));
    let started = events
        .iter()
        .any(|e| matches!(e, SessionEvent::TurnStarted { .. }));
    assert!(started, "the deferred first prompt correlates into a turn");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "a bound deferred send is not external input"
    );

    // The user line landed on main and the send is now matched (FIFO clear).
    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    let view = ix.thread_view(main).await.unwrap();
    assert!(view.iter().any(|m| m.uuid.as_str() == "u-1"));
    assert!(ix
        .store()
        .head_pending_send(&session_id)
        .await
        .unwrap()
        .is_none());
}

/// When the composer-first spawn cannot type its first prompt into the new
/// pane, the use case surfaces the dispatch error AND rolls the half-spawned
/// pane out of `pending`, so a later, unrelated `UserPromptSubmit` is not
/// mis-bound to it (it registers as external instead).
#[tokio::test]
async fn composer_first_send_rolls_back_pending_spawn_on_dispatch_failure() {
    use crate::error::Error;

    let ix = interactor_with_failing_tmux();

    // No session yet: the composer-first send spawns, then fails to type the
    // prompt into the pane. The error propagates.
    let err = ix
        .enqueue_send(SendTarget::NewSession, "first message", None)
        .await
        .expect_err("a failed first-prompt dispatch must propagate");
    assert!(matches!(err, Error::Tmux(_)));

    // The spawn was rolled back: no pending spawn remains, so a later hook
    // carrying any session id finds none to bind and is treated as an external,
    // closed session rather than binding to the abandoned pane.
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "the failed spawn left no pending entry behind"
    );
    let (events, _) = ix
        .on_user_prompt_submit(submit_in(
            "sess-late",
            "/work/delta-1/t.jsonl",
            "/work/delta-1",
            "typed in claude",
        ))
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "no pending spawn remained, so the hook is external input"
    );
    assert!(
        ix.pane_for_session(&SessionId::from("sess-late"))
            .await
            .is_none(),
        "the rolled-back spawn must not bind a later session"
    );
}

/// A `UserPromptSubmit` carrying a pending spawn's Delta-minted session id binds
/// that spawn (pending → bound) and registers the session.
#[tokio::test]
async fn user_prompt_binds_pending_spawn_by_session_id() {
    let ix = interactor();
    // Cold-start spawn (no first prompt).
    ix.new_session().await.unwrap();

    // Delta pinned the conversation's session id at spawn time; read it back.
    let session_id = ix.pending_session_ids().await.remove(0);

    // The spawn is not yet open under that session id.
    assert!(ix.pane_for_session(&session_id).await.is_none());

    // A hook reporting the pinned session id binds and registers. The cwd is
    // unrelated to binding now, so it can be anything.
    ix.on_user_prompt_submit(submit_in(
        session_id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();

    // Now bound: the pane is the spawn's pane, and the session row exists.
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        Some("delta-1:0.0".to_owned())
    );
    assert!(ix.store().session(&session_id).await.unwrap().is_some());
}

/// Two pending spawns that SHARE the same working directory each still bind to
/// the right session, because correlation is keyed by the Delta-minted session
/// id (pinned via `claude --session-id`), not by the cwd. This is the regression
/// guard for a future where the user picks a real project directory as the
/// session cwd: two spawns may then share a cwd without mis-correlating.
#[tokio::test]
async fn same_workdir_spawns_bind_to_their_own_session_each() {
    let ix = interactor();
    ix.new_session().await.unwrap(); // delta-1
    ix.new_session().await.unwrap(); // delta-2

    // Read back the two pinned session ids, in spawn order.
    let ids = ix.pending_session_ids().await;
    assert_eq!(ids.len(), 2, "two spawns are pending");
    let (id1, id2) = (ids[0].clone(), ids[1].clone());
    assert_ne!(id1, id2, "each spawn mints a distinct session id");

    // Fire the hooks in the OPPOSITE order to the spawn order, and crucially
    // with the SAME shared cwd for both — so only the session id can resolve
    // which spawn each binds to.
    const SHARED_CWD: &str = "/work/project";
    ix.on_user_prompt_submit(submit_in(
        id2.as_str(),
        "/work/project/t2.jsonl",
        SHARED_CWD,
        "hi",
    ))
    .await
    .unwrap();
    ix.on_user_prompt_submit(submit_in(
        id1.as_str(),
        "/work/project/t1.jsonl",
        SHARED_CWD,
        "hi",
    ))
    .await
    .unwrap();

    // Each session bound to its own spawn's pane despite the shared cwd.
    assert_eq!(
        ix.pane_for_session(&id1).await,
        Some("delta-1:0.0".to_owned()),
    );
    assert_eq!(
        ix.pane_for_session(&id2).await,
        Some("delta-2:0.0".to_owned()),
    );
}

/// `open_session` resumes a closed known session: it spawns `claude --resume
/// <id>` (asserted via the recorded argv), binds it, and a subsequent send uses
/// the normal pre-dispatch pending_send path into the resumed pane.
#[tokio::test]
async fn open_session_resumes_with_resume_argv_then_send_uses_normal_path() {
    let ix = interactor();
    // Register a known-but-closed session (an external claude in /elsewhere).
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");

    ix.open_session(&id).await.unwrap();

    // The resume spawned `claude --resume sess-R` in the session's stored cwd.
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    let resume = created
        .iter()
        .find(|c| c.command.iter().any(|a| a == "--resume"))
        .expect("a resume spawn was recorded");
    assert_eq!(
        resume.command,
        vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            TEST_SETTINGS_PATH.to_owned(),
            "--resume".to_owned(),
            "sess-R".to_owned()
        ],
    );
    assert_eq!(resume.workdir, "/elsewhere", "resumes in the stored cwd");
    let pane = ix.pane_for_session(&id).await.expect("now open");

    // A subsequent send writes the pending_send (normal path) and dispatches
    // into the resumed pane.
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let pending = ix
        .enqueue_send(to(main), "after resume", None)
        .await
        .unwrap();
    assert_ne!(pending.id, 0, "a real pending_send row was written");
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert!(
        sent.iter().any(|(p, t)| p == &pane && t == "after resume"),
        "the send dispatched into the resumed pane"
    );
}

/// `enqueue_send` against a known-but-*closed* session resumes it as part of the
/// send (the documented "Closed" branch): `ensure_open` finds no live pane, so
/// it spawns `claude --resume <id>` and then dispatches the message into the
/// freshly-resumed pane on the normal path — all within the single
/// `enqueue_send` call, with no prior explicit `open_session`. This pins the
/// resume-within-send wiring, which the test above only exercises after a
/// separate `open_session` (the already-open branch of `ensure_open`).
#[tokio::test]
async fn enqueue_send_resumes_a_closed_session_then_dispatches() {
    let ix = interactor();
    // Register a known-but-closed session (an external claude in /elsewhere):
    // it has a store row but no live pane.
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");

    let main = ix.store().main_thread_id(&id).await.unwrap();
    let pending = ix
        .enqueue_send(to(main), "after resume", None)
        .await
        .unwrap();
    assert_ne!(pending.id, 0, "a real pending_send row was written");

    // The send resumed the session: a `claude --resume sess-R` spawn was
    // recorded in the stored cwd, with no prior explicit open_session call.
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    let resume = created
        .iter()
        .find(|c| c.command.iter().any(|a| a == "--resume"))
        .expect("the send resumed the closed session");
    assert_eq!(
        resume.command,
        vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            TEST_SETTINGS_PATH.to_owned(),
            "--resume".to_owned(),
            "sess-R".to_owned()
        ],
    );
    assert_eq!(resume.workdir, "/elsewhere", "resumes in the stored cwd");

    // The session is now open and the message was dispatched into its pane.
    let pane = ix.pane_for_session(&id).await.expect("now open after send");
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert!(
        sent.iter().any(|(p, t)| p == &pane && t == "after resume"),
        "the send dispatched into the resumed pane"
    );
}

/// Reproduces the DB-behind precondition that produced the resume bug: a known
/// session whose transcript already holds prior user history, but whose DB
/// message rows and read cursor have not caught up to it yet (a cold/just-
/// restored DB, or any DB-behind-transcript state). In that state
/// `latest_user_thread` reports `None`, even though the user really was in a
/// thread — the stale value that mis-seeds thread context on the first
/// post-resume prompt.
#[tokio::test]
async fn db_behind_transcript_reports_no_latest_user_thread() {
    let ix = interactor();
    // Register a known-but-closed session. At registration its transcript is
    // empty, so the cursor is 0 and no message rows exist.
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");

    // Prior history is written to the transcript WITHOUT syncing: the DB is now
    // behind the transcript (message table empty, cursor 0).
    ix.transcript_fake()
        .push_to("/elsewhere/t.jsonl", user_line("u-prior", "prior prompt"));
    ix.transcript_fake()
        .push_to("/elsewhere/t.jsonl", assistant_line("a-prior", "prior reply"));

    // Precondition: the DB-behind state makes `latest_user_thread` report `None`
    // even though the transcript holds a prior user line.
    assert!(
        ix.store().latest_user_thread(&id).await.unwrap().is_none(),
        "DB behind transcript: no user row yet, so the latest user thread is unknown"
    );
    assert_eq!(
        ix.store().message_count(&id).await.unwrap(),
        0,
        "no prior history ingested yet"
    );
}

/// The root fix: `open_session` catches the DB up to the existing transcript
/// before returning, so the resume's first prompt resolves thread context
/// against the user's real last thread instead of a DB-behind `None`. After the
/// open, the prior history is ingested and `latest_user_thread` reports the
/// prior user line's thread.
#[tokio::test]
async fn open_session_syncs_existing_transcript_so_latest_user_thread_is_known() {
    let ix = interactor();
    // Register a known-but-closed session (empty transcript at registration).
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    let main = ix.store().main_thread_id(&id).await.unwrap();

    // Prior history exists in the transcript but is not yet ingested (DB behind).
    ix.transcript_fake()
        .push_to("/elsewhere/t.jsonl", user_line("u-prior", "prior prompt"));
    ix.transcript_fake()
        .push_to("/elsewhere/t.jsonl", assistant_line("a-prior", "prior reply"));
    assert!(
        ix.store().latest_user_thread(&id).await.unwrap().is_none(),
        "precondition: DB behind transcript"
    );

    // Resume the session. The catch-up sync runs as part of the open.
    ix.open_session(&id).await.unwrap();

    // The DB is now caught up: the prior user line is ingested and reported as
    // the latest user thread, so the first post-resume prompt sees the real
    // previous thread rather than `None`.
    assert_eq!(
        ix.store().latest_user_thread(&id).await.unwrap(),
        Some(main),
        "the prior user line is now the known latest user thread"
    );
    let view = ix.thread_view(main).await.unwrap();
    let uuids: Vec<&str> = view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(uuids.contains(&"u-prior"), "prior user line ingested on open");
    assert!(
        uuids.contains(&"a-prior"),
        "prior assistant line ingested on open"
    );
}

/// Register a known-but-closed session that has a prior *branch* turn pending
/// (a child thread plus a queued branch send matching `prior branch prompt`),
/// returning the interactor and the `(session, main, child)` ids. The branch
/// send and child thread are written via the store directly, NOT through
/// `enqueue_send`, so the closed session is not resumed yet (going through
/// `enqueue_send` would open it early and trip the double-open guard on the
/// explicit `open_session` under test).
async fn closed_session_with_pending_branch() -> (
    Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace>,
    SessionId,
    ThreadId,
    ThreadId,
) {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let parent = MessageUuid::from("uuid-parent");
    let child = ix
        .store()
        .create_thread(&id, "prior branch prompt", Some(main), Some(&parent))
        .await
        .unwrap()
        .id;
    ix.store()
        .enqueue_send(&id, child, Some(&parent), "prior branch prompt", None)
        .await
        .unwrap();
    (ix, id, main, child)
}

/// The thread a given ingested message landed on, by uuid.
fn ingested_thread(
    ix: &Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace>,
    uuid: &str,
) -> Option<ThreadId> {
    ix.store()
        .inner
        .lock()
        .unwrap()
        .messages
        .iter()
        .find(|m| m.uuid.as_str() == uuid)
        .map(|m| m.thread_id)
}

/// `carry_thread` regression — the PRE-FIX behaviour this fix removes.
///
/// `sync_transcript` seeds `carry_thread` from
/// `latest_user_thread().unwrap_or(main)`. When the DB is behind the transcript
/// at the resume boundary, `latest_user_thread` is `None`, so the seed defaults
/// to `main`. A non-user line that leads the synced batch — before any user
/// line in it re-corrects `carry_thread` — is then mis-attributed to `main`,
/// even though it is the tail of the user's prior (branch) turn.
///
/// This drives that batch directly (no `open_session` catch-up) to pin the
/// mechanism the fix targets.
#[tokio::test]
async fn db_behind_mis_seeds_carry_thread_to_main_for_a_leading_non_user_line() {
    let (ix, id, main, _child) = closed_session_with_pending_branch().await;

    // The DB is behind: no user row yet, so the latest user thread is unknown.
    assert!(
        ix.store().latest_user_thread(&id).await.unwrap().is_none(),
        "precondition: DB behind transcript"
    );

    // A batch whose head is a non-user line (the tail of the prior branch turn),
    // with no user line in it to re-correct the carry thread.
    ix.transcript_fake()
        .push_to("/elsewhere/t.jsonl", assistant_line("a-lead", "leading reply"));
    ix.poll_transcript().await.unwrap();

    assert_eq!(
        ingested_thread(&ix, "a-lead"),
        Some(main),
        "with the DB behind, the leading non-user line is mis-attributed to main"
    );
}

/// The root fix closes the window above. `open_session` catches the DB up to
/// the prior branch turn before returning, so by the time the post-resume tail
/// batch is synced `latest_user_thread` is the branch. A non-user line leading
/// that batch then follows the branch carry thread, not `main`.
#[tokio::test]
async fn open_session_seeds_carry_thread_from_branch_so_leading_line_is_not_main() {
    let (ix, id, main, child) = closed_session_with_pending_branch().await;

    // The prior branch user line sits in the transcript, unsynced (DB behind).
    ix.transcript_fake().push_to(
        "/elsewhere/t.jsonl",
        user_line("u-branch", "prior branch prompt"),
    );
    assert!(
        ix.store().latest_user_thread(&id).await.unwrap().is_none(),
        "precondition: DB behind transcript"
    );

    // Resume: the catch-up sync ingests the prior branch turn, so the branch
    // becomes the known latest user thread.
    ix.open_session(&id).await.unwrap();
    assert_eq!(
        ix.store().latest_user_thread(&id).await.unwrap(),
        Some(child),
        "open caught the DB up to the prior branch user line"
    );

    // The post-resume tail now arrives as its own batch, leading with a
    // non-user line. It follows the branch carry thread, not main.
    ix.transcript_fake()
        .push_to("/elsewhere/t.jsonl", assistant_line("a-lead", "post-resume reply"));
    ix.poll_transcript().await.unwrap();
    assert_eq!(
        ingested_thread(&ix, "a-lead"),
        Some(child),
        "the leading non-user line follows the branch carry thread, not main"
    );

    // Nothing leaked onto main.
    let main_view = ix.thread_view(main).await.unwrap();
    assert!(
        main_view.is_empty(),
        "no line was mis-attributed to main, got: {:?}",
        main_view.iter().map(|m| m.uuid.as_str()).collect::<Vec<_>>()
    );
}

/// Opening an already-open session does not spawn a second pane (double-open
/// guard): it routes to the existing one.
#[tokio::test]
async fn open_session_is_a_noop_when_already_open() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");

    ix.open_session(&id).await.unwrap();
    let first_pane = ix.pane_for_session(&id).await.unwrap();
    let created_after_first = ix.tmux_fake().created.lock().unwrap().len();

    // A second open is a no-op: same pane, no new spawn.
    ix.open_session(&id).await.unwrap();
    assert_eq!(ix.pane_for_session(&id).await.unwrap(), first_pane);
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        created_after_first,
        "no second pane spawned for an already-open session"
    );
}

/// `clear_session_input` wipes the open session's pane via the driver, and is a
/// no-op (no driver call, no error) when the session is not open. This pins the
/// clear-on-attach path the PTY bridge uses before a fresh attach.
#[tokio::test]
async fn clear_session_input_clears_open_pane_and_noops_when_closed() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_in(
        "sess-C",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-C");

    // Closed: clearing is a no-op that records no driver call.
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");
    ix.clear_session_input(&id).await.unwrap();
    assert!(
        ix.tmux_fake().cleared.lock().unwrap().is_empty(),
        "a closed session has no live pane to clear"
    );

    // Open it, then clearing targets the bound pane.
    ix.open_session(&id).await.unwrap();
    let pane = ix.pane_for_session(&id).await.expect("now open");
    ix.clear_session_input(&id).await.unwrap();
    assert_eq!(
        ix.tmux_fake().cleared.lock().unwrap().clone(),
        vec![pane],
        "the open session's pane was cleared exactly once"
    );
}

/// Opening a session id that does not exist in the store is rejected with
/// `SessionNotFound` (the variant the API layer maps to 404), and no pane is
/// spawned. This is the only code path that produces `SessionNotFound`, so it
/// pins both the error and the reason its 404 mapping exists.
#[tokio::test]
async fn open_session_unknown_id_is_session_not_found() {
    use crate::error::Error;

    let ix = interactor();
    let err = ix
        .open_session(&SessionId::from("ghost"))
        .await
        .expect_err("opening a non-existent session must be rejected");
    assert!(
        matches!(err, Error::SessionNotFound(id) if id == "ghost"),
        "the missing id is surfaced as SessionNotFound"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a rejected open must not spawn a pane"
    );
}

/// Opening a known-but-closed session whose transcript file is gone is rejected
/// with `ResumeUnavailable` (which the API layer maps to 409): `claude --resume`
/// would have nothing to replay, so the gate refuses before minting a token,
/// writing settings, or spawning. No pane is created and the session stays
/// closed.
#[tokio::test]
async fn open_session_missing_transcript_is_resume_unavailable() {
    use crate::error::Error;

    let ix = interactor();
    // Register a known-but-closed session, then model its transcript as removed.
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    ix.transcript_fake().mark_missing("/elsewhere/t.jsonl");
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");

    let err = ix
        .open_session(&id)
        .await
        .expect_err("a missing transcript makes resume impossible");
    assert!(
        matches!(err, Error::ResumeUnavailable(ref s) if s == "sess-R"),
        "the session id is surfaced as ResumeUnavailable, got: {err:?}"
    );

    // Nothing was spawned and no settings were written: the gate runs before any
    // of that, and the session remains closed.
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a resume-unavailable open must not spawn a pane"
    );
    assert!(
        ix.workspace_fake().written.lock().unwrap().is_empty(),
        "a resume-unavailable open must not write session settings"
    );
    assert!(
        ix.pane_for_session(&id).await.is_none(),
        "the session stays closed"
    );
}

/// A Send to a closed session whose transcript is gone fails before any pending
/// row is written: `ensure_open` resumes via `open_session`, which now refuses
/// with `ResumeUnavailable`, so `enqueue_into_open` never runs. This is the
/// fix for the "stuck waiting indicator" — without an optimistic pending row,
/// the UI has nothing to leave hanging.
#[tokio::test]
async fn send_to_closed_session_with_missing_transcript_writes_no_pending_row() {
    use crate::error::Error;

    let ix = interactor();
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    ix.transcript_fake().mark_missing("/elsewhere/t.jsonl");
    let main = ix.store().main_thread_id(&id).await.unwrap();

    let err = ix
        .enqueue_send(to(main), "after resume", None)
        .await
        .expect_err("a send to a resume-impossible session must fail");
    assert!(
        matches!(err, Error::ResumeUnavailable(ref s) if s == "sess-R"),
        "the failure propagates as ResumeUnavailable, got: {err:?}"
    );

    // The key assertion: no optimistic pending row sits at the FIFO head waiting
    // for a hook that will never fire.
    assert!(
        ix.store().head_pending_send(&id).await.unwrap().is_none(),
        "no pending send row was enqueued"
    );
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "no keystrokes were dispatched"
    );
    assert!(
        ix.pane_for_session(&id).await.is_none(),
        "the session stays closed"
    );
}

/// A branch send targeting a thread that does not exist is rejected with
/// `ThreadNotFound`. A branch send always names the parent thread it hangs off,
/// so with no such thread (no session has been registered) there is nothing to
/// branch from — and it must not silently spawn a fresh session.
#[tokio::test]
async fn branch_send_to_unknown_thread_is_thread_not_found() {
    use crate::error::Error;

    let ix = interactor();
    let parent = MessageUuid::from("uuid-parent");
    let err = ix
        .enqueue_send(branch_off(ThreadId(1), &parent), "branch text", None)
        .await
        .expect_err("a branch send needs an existing parent thread");
    assert!(matches!(err, Error::ThreadNotFound(_)));
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a rejected branch send must not spawn a session"
    );
}

/// Closing a *known* session that is not open is a no-op: no pane is killed and
/// no error is raised, so a stale close from the browser is harmless.
#[tokio::test]
async fn close_session_known_but_not_open_is_a_noop() {
    let ix = interactor();
    // Register a known-but-closed session (an external claude): it has a store
    // row but no live pane.
    ix.on_user_prompt_submit(submit_in(
        "sess-closed",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-closed");
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");

    ix.close_session(&id)
        .await
        .expect("closing a known non-open session is a no-op, not an error");
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "no pane is killed when nothing was open"
    );
}

/// Closing an *unknown* session id is rejected with `SessionNotFound` (the
/// variant the API layer maps to 404), symmetric with `open_session`. This keeps
/// "already closed" distinguishable from "no such session" so a stale id does not
/// silently succeed, and no pane is killed.
#[tokio::test]
async fn close_session_unknown_id_is_session_not_found() {
    use crate::error::Error;

    let ix = interactor();
    let err = ix
        .close_session(&SessionId::from("ghost"))
        .await
        .expect_err("closing a non-existent session must be rejected");
    assert!(
        matches!(err, Error::SessionNotFound(id) if id == "ghost"),
        "the missing id is surfaced as SessionNotFound"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "a rejected close must not kill a pane"
    );
}

/// A `UserPromptSubmit` for an unknown id with NO matching pending spawn is an
/// external claude: it registers a closed data session (no open pane) and emits
/// external input, without panicking.
#[tokio::test]
async fn unknown_session_without_pending_spawn_registers_external_closed() {
    let ix = interactor();
    ix.transcript_fake()
        .push_to("/outside/t.jsonl", user_line("u-x", "typed outside"));

    let (events, _) = ix
        .on_user_prompt_submit(submit_in(
            "sess-X",
            "/outside/t.jsonl",
            "/outside",
            "typed outside",
        ))
        .await
        .unwrap();

    // Registered, but closed (no live pane), and reported as external input.
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-X"),
    }));
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::ExternalInput { prompt, .. } if prompt == "typed outside"
    )));
    assert!(
        ix.pane_for_session(&SessionId::from("sess-X"))
            .await
            .is_none(),
        "an external session has no open pane"
    );
}

/// `close_session` kills the pane (recorded by the fake) and removes it from the
/// registry, while the session data remains in the store.
#[tokio::test]
async fn close_session_kills_the_pane_and_keeps_the_data() {
    let ix = interactor();
    // Spawn and bind a session.
    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(
        ix.pane_for_session(&id).await.is_some(),
        "open before close"
    );

    ix.close_session(&id).await.unwrap();

    // The pane was killed by token, and the session is no longer open.
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-1".to_owned()],
    );
    assert!(ix.pane_for_session(&id).await.is_none(), "closed");
    // The data session remains.
    assert!(ix.store().session(&id).await.unwrap().is_some());
}

/// `frame_locator_context` wraps a non-empty quote with provenance framing and
/// the quote delimited so the frame and the quote stay distinguishable.
#[test]
fn frame_locator_context_frames_a_quote() {
    let framed =
        super::frame_locator_context("the main channel").expect("a non-empty quote is framed");
    assert_eq!(
        framed,
        "The user is replying to this passage they selected from earlier in the conversation:\n\"the main channel\""
    );
}

/// Surrounding whitespace is trimmed before the quote is framed.
#[test]
fn frame_locator_context_trims_the_quote() {
    let framed = super::frame_locator_context("  spaced  ").expect("a non-blank quote is framed");
    assert_eq!(
        framed,
        "The user is replying to this passage they selected from earlier in the conversation:\n\"spaced\""
    );
}

/// An empty or whitespace-only quote yields `None`, so nothing is injected.
#[test]
fn frame_locator_context_returns_none_for_blank_quote() {
    assert!(super::frame_locator_context("").is_none());
    assert!(super::frame_locator_context("   \n\t ").is_none());
}

/// A selected passage may itself contain double quotes and span multiple lines.
/// Only the surrounding whitespace is trimmed; the interior is embedded
/// verbatim, with no escaping of the delimiters. The frame is a prose hint for
/// the model, not a strict grammar, so this pins the shipped behaviour down
/// rather than asserting any escaping.
#[test]
fn frame_locator_context_embeds_quotes_and_newlines_verbatim() {
    let framed = super::frame_locator_context("  she said \"go\"\nthen left  ")
        .expect("a non-blank quote is framed");
    assert_eq!(
        framed,
        "The user is replying to this passage they selected from earlier in the conversation:\n\"she said \"go\"\nthen left\""
    );
}

/// A spawn skips tmux session names that already exist (surviving `delta-<n>`
/// sessions from a previous server run), so it never fails with tmux's
/// "duplicate session". The minter resets to `delta-1` on each start, so without
/// this a restart that left old panes alive would re-mint a colliding name.
#[tokio::test]
async fn spawn_skips_tmux_session_names_already_in_use() {
    let ix = Interactor::new(
        FakeTmux {
            // Two panes from a previous run survived the restart.
            live: Mutex::new(vec!["delta-1".to_owned(), "delta-2".to_owned()]),
            ..Default::default()
        },
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        TEST_WORKDIR_BASE,
        TEST_SETTINGS_JSON,
        TEST_SETTINGS_PATH,
    );

    let token = ix.new_session().await.expect("spawn does not collide");

    assert_eq!(
        token.as_str(),
        "delta-3",
        "the spawn skipped the two surviving names and minted the next free one",
    );
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "exactly one session was created");
    assert_eq!(created[0].name, "delta-3", "created under the free name");
    assert_eq!(created[0].workdir, "/work/delta-3", "<base>/<free token>");
}

// Helper accessors used only in tests to reach into the fakes the interactor owns.
impl Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace> {
    fn transcript_fake(&self) -> &FakeTranscript {
        self.transcript()
    }

    fn tmux_fake(&self) -> &FakeTmux {
        self.tmux()
    }

    fn workspace_fake(&self) -> &FakeWorkspace {
        self.workspace()
    }
}

impl FakeTranscript {
    /// Append a parsed message as the next line of the default transcript.
    fn push(&self, line: TranscriptMessage) {
        self.push_to(DEFAULT_TRANSCRIPT_PATH, line);
    }

    /// Append a parsed message as the next line of a specific transcript path.
    fn push_to(&self, path: &str, line: TranscriptMessage) {
        self.by_path
            .lock()
            .unwrap()
            .entry(path.to_owned())
            .or_default()
            .push(Some(line));
    }

    /// Mark a transcript path as absent, so [`Transcript::exists`] reports
    /// `false` for it — modelling a removed transcript that makes
    /// `claude --resume` impossible.
    fn mark_missing(&self, path: &str) {
        self.missing.lock().unwrap().push(path.to_owned());
    }

    /// Append a line that produces no message but still occupies a line and
    /// advances the cursor (e.g. Claude Code's `file-history-snapshot`).
    fn push_skipped_line(&self) {
        self.by_path
            .lock()
            .unwrap()
            .entry(DEFAULT_TRANSCRIPT_PATH.to_owned())
            .or_default()
            .push(None);
    }
}
