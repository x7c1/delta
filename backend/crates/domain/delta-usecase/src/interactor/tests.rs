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
use crate::Interactor;

#[derive(Default)]
struct FakeTmux {
    sent: Mutex<Vec<String>>,
    /// When set, `send_line` fails instead of recording the line, simulating a
    /// dispatch failure into the pane.
    fail: bool,
    /// Whether a session currently "exists" for `has_session`. Toggled to true
    /// by `create_session`.
    has_session: Mutex<bool>,
    /// The `(workdir, command)` pairs `create_session` was called with.
    created: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl TmuxDriver for FakeTmux {
    async fn has_session(&self) -> Result<bool> {
        Ok(*self.has_session.lock().unwrap())
    }

    async fn create_session(&self, workdir: &str, command: &str) -> Result<()> {
        self.created
            .lock()
            .unwrap()
            .push((workdir.to_owned(), command.to_owned()));
        *self.has_session.lock().unwrap() = true;
        Ok(())
    }

    async fn send_line(&self, text: &str) -> Result<()> {
        if self.fail {
            return Err(crate::error::Error::Tmux("dispatch failed".into()));
        }
        self.sent.lock().unwrap().push(text.to_owned());
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

fn interactor() -> Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace> {
    Interactor::new(
        FakeTmux::default(),
        FakeTranscript::default(),
        FakeStore::default(),
        FakeWorkspace::default(),
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
async fn ensure_session_creates_and_writes_settings_when_absent() {
    let ix = interactor();

    let status = ix
        .ensure_session("/work/session", "claude", r#"{"hooks":{}}"#)
        .await
        .unwrap();

    // A fresh session reports `Starting`, was created with the given workdir and
    // command, and had its settings written first.
    assert_eq!(status, SessionLifecycle::Starting);
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(
        created,
        vec![("/work/session".to_owned(), "claude".to_owned())]
    );
    let written = ix.workspace_fake().written.lock().unwrap().clone();
    assert_eq!(
        written,
        vec![("/work/session".to_owned(), r#"{"hooks":{}}"#.to_owned())]
    );
}

#[tokio::test]
async fn ensure_session_reuses_existing_session_idempotently() {
    let ix = interactor();

    // First call creates the session.
    ix.ensure_session("/work/session", "claude", "{}")
        .await
        .unwrap();
    // Second call finds it already up: reuse, no second create, no second write.
    let status = ix
        .ensure_session("/work/session", "claude", "{}")
        .await
        .unwrap();

    assert_eq!(status, SessionLifecycle::Ready);
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        1,
        "an existing session must not be recreated"
    );
    assert_eq!(
        ix.workspace_fake().written.lock().unwrap().len(),
        1,
        "settings must not be rewritten when the session is reused"
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

/// The temporary single-session shim behind the still-single REST surface
/// (`current_session` / `threads`) collapses the multi-session store down to the
/// most-recently-created session. With several registered, it must resolve to
/// the last one, not the first — pinning `resolve_single_session`'s choice so a
/// `.last()` → `.first()` slip is caught. With none registered it returns `None`.
#[tokio::test]
async fn single_session_shim_resolves_most_recently_created() {
    let ix = interactor();

    // No session yet: the shim resolves cleanly to nothing.
    assert!(ix.current_session().await.unwrap().is_none());
    assert!(ix.threads().await.unwrap().is_empty());

    // Register two sessions in order; sess-2 is the most-recently-created.
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "seed"))
        .await
        .unwrap();

    let (session, _main) = ix
        .current_session()
        .await
        .unwrap()
        .expect("a session resolves once one is registered");
    assert_eq!(
        session.id.as_str(),
        "sess-2",
        "the shim resolves to the most-recently-created session"
    );

    // `threads` is scoped to that same resolved session: only its `main` thread.
    let threads = ix.threads().await.unwrap();
    assert!(
        threads.iter().all(|t| t.session_id.as_str() == "sess-2"),
        "threads must belong to the resolved (most-recent) session only"
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
        .enqueue_send(main, "hello world", Some("[quote]"), None)
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
        .enqueue_send(main, "branch text", None, Some(&parent))
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
        .enqueue_send(main, "hello world", None, None)
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
        .enqueue_send(main, "branch text", None, Some(&parent))
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
        .enqueue_send(main, "branch text", Some("quoted line"), Some(&parent))
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
            main,
            "branch text",
            Some("  the quoted source line  "),
            Some(&parent),
        )
        .await
        .unwrap();
    let child = ix.store().thread(pending.thread_id).await.unwrap().unwrap();
    assert_eq!(child.title, "the quoted source line");

    // With no quote, the title falls back to "untitled".
    let pending2 = ix
        .enqueue_send(main, "branch text 2", None, Some(&parent))
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
        .enqueue_send(ThreadId(999), "hello", None, None)
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
        .enqueue_send(main, "never delivered", None, None)
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
    ix.enqueue_send(main, "turn one", None, None).await.unwrap();
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
    ix.enqueue_send(main, "turn two", None, None).await.unwrap();
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
    ix.enqueue_send(main, "hello world", None, None)
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
