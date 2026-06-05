//! Interactor use-case tests against in-memory fakes.

use std::sync::Mutex;

use async_trait::async_trait;
use delta_model::{
    ContentBlock, Message, MessageUuid, PendingSend, PendingSendStatus, PermissionRequest,
    PermissionStatus, Role, Session, SessionId, SessionStatus, Thread, ThreadId,
};

use crate::error::Result;
use crate::ports::{
    NewSession, SessionEvent, SessionStore, TmuxDriver, Transcript, TranscriptMessage,
    UserPromptSubmitHook,
};
use crate::Interactor;

#[derive(Default)]
struct FakeTmux {
    sent: Mutex<Vec<String>>,
}

#[async_trait]
impl TmuxDriver for FakeTmux {
    async fn send_line(&self, text: &str) -> Result<()> {
        self.sent.lock().unwrap().push(text.to_owned());
        Ok(())
    }
}

#[derive(Default)]
struct FakeTranscript {
    lines: Mutex<Vec<TranscriptMessage>>,
}

#[async_trait]
impl Transcript for FakeTranscript {
    async fn read_from(&self, _path: &str, from_seq: usize) -> Result<Vec<TranscriptMessage>> {
        let lines = self.lines.lock().unwrap();
        Ok(lines.iter().skip(from_seq).cloned().collect())
    }
}

#[derive(Default)]
struct FakeStoreInner {
    session: Option<Session>,
    threads: Vec<Thread>,
    next_thread_id: i64,
    sends: Vec<PendingSend>,
    next_send_id: i64,
    messages: Vec<Message>,
    permissions: Vec<PermissionRequest>,
    next_perm_id: i64,
}

#[derive(Default)]
struct FakeStore {
    inner: Mutex<FakeStoreInner>,
}

#[async_trait]
impl SessionStore for FakeStore {
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
        let mut g = self.inner.lock().unwrap();
        let session = Session {
            id: new.id.clone(),
            cwd: new.cwd,
            transcript_path: new.transcript_path,
            title: None,
            status: SessionStatus::Active,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        g.session = Some(session.clone());
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

    async fn current_session(&self) -> Result<Option<Session>> {
        Ok(self.inner.lock().unwrap().session.clone())
    }

    async fn main_thread_id(&self, _session_id: &SessionId) -> Result<ThreadId> {
        let g = self.inner.lock().unwrap();
        Ok(g.threads.iter().find(|t| t.title == "main").unwrap().id)
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
        Ok(g.messages.iter().filter(|m| &m.session_id == session_id).count())
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
    }
}

fn interactor() -> Interactor<FakeTmux, FakeTranscript, FakeStore> {
    Interactor::new(FakeTmux::default(), FakeTranscript::default(), FakeStore::default())
}

fn submit(text: &str) -> UserPromptSubmitHook {
    UserPromptSubmitHook {
        prompt: text.into(),
        session_id: SessionId::from("sess-1"),
        transcript_path: "/tmp/t.jsonl".into(),
        cwd: "/work".into(),
    }
}

#[tokio::test]
async fn first_submit_registers_session() {
    let ix = interactor();
    let (events, _) = ix.on_user_prompt_submit(submit("hi")).await.unwrap();
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-1"),
    }));
    assert!(ix.store().current_session().await.unwrap().is_some());
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
    let pending = ix.enqueue_send(main, "hello world", Some("[quote]"), None).await.unwrap();
    assert_eq!(pending.status, PendingSendStatus::Pending);

    // The transcript now contains the matching user line.
    ix.transcript_fake().push(user_line("uuid-1", "hello world"));

    let (events, additional) = ix.on_user_prompt_submit(submit("hello world")).await.unwrap();
    assert_eq!(additional.as_deref(), Some("[quote]"));
    let started = events
        .iter()
        .find_map(|e| match e {
            SessionEvent::TurnStarted { matched_uuid, pending_send_id, .. } => {
                Some((matched_uuid.clone(), *pending_send_id))
            }
            _ => None,
        })
        .expect("turn started event");
    assert_eq!(started.0, MessageUuid::from("uuid-1"));
    assert_eq!(started.1, pending.id);

    // Marked matched; no longer the head.
    let head = ix.store().head_pending_send(&SessionId::from("sess-1")).await.unwrap();
    assert!(head.is_none());
}

#[tokio::test]
async fn unmatched_prompt_is_external_input() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    ix.transcript_fake().push(user_line("u-ext", "typed directly"));

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

// Helper accessor used only in tests to push transcript lines onto the fake.
impl Interactor<FakeTmux, FakeTranscript, FakeStore> {
    fn transcript_fake(&self) -> &FakeTranscript {
        self.transcript()
    }
}

impl FakeTranscript {
    fn push(&self, line: TranscriptMessage) {
        self.lines.lock().unwrap().push(line);
    }
}
