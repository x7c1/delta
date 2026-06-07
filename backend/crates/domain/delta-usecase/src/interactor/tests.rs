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
    NewSession, SessionEvent, SessionLifecycle, SessionStore, TmuxDriver, Transcript,
    TranscriptMessage, TranscriptRead, UserPromptSubmitHook, Workspace,
};
use crate::{Interactor, SendTarget};

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

    async fn kill_session(&self, name: &str) -> Result<()> {
        self.killed.lock().unwrap().push(name.to_owned());
        self.live.lock().unwrap().retain(|n| n != name);
        Ok(())
    }
}

/// Records the session settings written, so tests can assert the workdir and the
/// rendered JSON the server passed in.
#[derive(Default)]
struct FakeWorkspace {
    written: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl Workspace for FakeWorkspace {
    async fn write_session_settings(&self, workdir: &str, settings_json: &str) -> Result<()> {
        self.written
            .lock()
            .unwrap()
            .push((workdir.to_owned(), settings_json.to_owned()));
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

/// The base working directory the test interactor spawns sessions under.
const TEST_WORKDIR_BASE: &str = "/work";

/// The hook settings JSON the test interactor writes into each spawn's workdir.
const TEST_SETTINGS_JSON: &str = r#"{"hooks":{}}"#;

fn interactor() -> Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace> {
    Interactor::new(
        FakeTmux::default(),
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
        TEST_WORKDIR_BASE,
        TEST_SETTINGS_JSON,
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
    // per-token workdir under the base, with the settings written there first.
    assert_eq!(status, SessionLifecycle::Starting);
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session was spawned");
    assert_eq!(created[0].name, "delta-1", "named after the minted token");
    assert_eq!(created[0].workdir, "/work/delta-1", "<base>/<token>");
    assert_eq!(
        created[0].command,
        vec!["claude".to_owned()],
        "plain claude"
    );
    let written = ix.workspace_fake().written.lock().unwrap().clone();
    assert_eq!(
        written,
        vec![("/work/delta-1".to_owned(), TEST_SETTINGS_JSON.to_owned())]
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

    // Spawn, then bind it to a session id via a hook in its workdir.
    ix.ensure_session().await.unwrap();
    ix.on_user_prompt_submit(submit_in(
        "sess-B",
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(
        ix.pane_for_session(&SessionId::from("sess-B"))
            .await
            .is_some(),
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

/// `list_sessions` lists every registered session in creation order, each
/// annotated with its live (open) state and `main` thread id; `threads_for`
/// scopes the thread tree to a single session by id.
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
    assert_eq!(ids, vec!["sess-1", "sess-2"], "ordered by creation");
    assert!(
        listings.iter().all(|l| !l.open),
        "externally-registered sessions are closed (no live pane)"
    );
    assert!(
        listings.iter().all(|l| l.main_thread_id.value() > 0),
        "every listing carries its main thread id"
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

/// The `open` flag tracks live state: a session with a bound pane lists as
/// `open: true`, and once closed it lists as `open: false` while still present.
/// The annotated-as-closed test above only pins the closed side, so this pins
/// the open side (and the open→closed transition) that the API surfaces.
#[tokio::test]
async fn list_sessions_marks_a_bound_session_open_and_a_closed_one_not() {
    let ix = interactor();

    // Spawn and bind a session: it now has a live pane.
    ix.new_session().await.unwrap();
    ix.on_user_prompt_submit(submit_in(
        "sess-open",
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-open");
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
    assert_eq!(additional, super::frame_locator_context("[quote]"));
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
    // quote is still returned (text-based), but nothing is attributed yet.
    let (events, additional) = ix
        .on_user_prompt_submit(submit("branch text"))
        .await
        .unwrap();
    assert_eq!(additional, super::frame_locator_context("quoted line"));
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

/// A submit hook for an explicit cwd, used by the spawn-binding tests where the
/// hook's cwd is the correlation key to a pending spawn.
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

    // The first UserPromptSubmit arrives in the spawn's workdir. It binds the
    // spawn to the now-known session id, registers the session, and writes the
    // deferred pending_send BEFORE attribution — so the user line correlates.
    ix.transcript_fake()
        .push_to("/work/delta-1/t.jsonl", user_line("u-1", "first message"));
    let (events, _) = ix
        .on_user_prompt_submit(submit_in(
            "sess-A",
            "/work/delta-1/t.jsonl",
            "/work/delta-1",
            "first message",
        ))
        .await
        .unwrap();

    // The session registered and the first turn started (the deferred send was
    // written and matched the user line).
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-A"),
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
    let main = ix
        .store()
        .main_thread_id(&SessionId::from("sess-A"))
        .await
        .unwrap();
    let view = ix.thread_view(main).await.unwrap();
    assert!(view.iter().any(|m| m.uuid.as_str() == "u-1"));
    assert!(ix
        .store()
        .head_pending_send(&SessionId::from("sess-A"))
        .await
        .unwrap()
        .is_none());
}

/// When the composer-first spawn cannot type its first prompt into the new
/// pane, the use case surfaces the dispatch error AND rolls the half-spawned
/// pane out of `pending`, so a later, unrelated `UserPromptSubmit` arriving in
/// that workdir is not mis-bound to it (it registers as external instead).
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

    // The spawn was rolled back: a hook later arriving in that same workdir
    // finds no pending spawn and is treated as an external, closed session
    // rather than binding to the abandoned pane.
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

/// A `UserPromptSubmit` for an unknown session id whose cwd matches a pending
/// spawn binds that spawn (pending → bound) and registers the session.
#[tokio::test]
async fn user_prompt_binds_pending_spawn_by_workdir() {
    let ix = interactor();
    // Cold-start spawn (no first prompt).
    ix.new_session().await.unwrap();

    // The spawn is not yet open under any session id.
    assert!(ix
        .pane_for_session(&SessionId::from("sess-A"))
        .await
        .is_none());

    // A hook arrives in the spawn's workdir; it binds and registers.
    ix.on_user_prompt_submit(submit_in(
        "sess-A",
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();

    // Now bound: the pane is the spawn's pane, and the session row exists.
    assert_eq!(
        ix.pane_for_session(&SessionId::from("sess-A")).await,
        Some("delta-1:0.0".to_owned())
    );
    assert!(ix
        .store()
        .session(&SessionId::from("sess-A"))
        .await
        .unwrap()
        .is_some());
}

/// Two pending spawns with DIFFERENT workdirs bind to the right session each:
/// the per-spawn unique-workdir guarantee makes the correlation exact.
#[tokio::test]
async fn two_pending_spawns_bind_to_the_right_session_each() {
    let ix = interactor();
    ix.new_session().await.unwrap(); // delta-1 → /work/delta-1
    ix.new_session().await.unwrap(); // delta-2 → /work/delta-2

    // Bind them in the opposite order to their spawn order, by workdir.
    ix.on_user_prompt_submit(submit_in(
        "sess-2",
        "/work/delta-2/t.jsonl",
        "/work/delta-2",
        "hi",
    ))
    .await
    .unwrap();
    ix.on_user_prompt_submit(submit_in(
        "sess-1",
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();

    assert_eq!(
        ix.pane_for_session(&SessionId::from("sess-1")).await,
        Some("delta-1:0.0".to_owned()),
    );
    assert_eq!(
        ix.pane_for_session(&SessionId::from("sess-2")).await,
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
    ix.on_user_prompt_submit(submit_in(
        "sess-C",
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-C");
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
