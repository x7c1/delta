//! Capability traits (ports) and the data they exchange.
//!
//! These traits describe what Delta needs from the outside world. The gateway
//! crates implement them; the [`crate::Interactor`] consumes them. Everything
//! here is expressed in terms of [`delta_model`] types only.

use async_trait::async_trait;
use serde::Serialize;

use delta_model::{
    ContentBlock, Message, MessageUuid, PendingSend, PermissionRequest, Role, Session, SessionId,
    Thread, ThreadId,
};

use crate::error::Result;

/// A parsed transcript line, before Delta assigns it a thread.
///
/// The transcript gateway produces these from the raw JSONL; the Interactor
/// turns them into [`Message`] values by attaching the active `thread_id` and
/// any known semantic parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptMessage {
    pub uuid: MessageUuid,
    pub role: Role,
    /// The transcript `parentUuid` (linear/model context order).
    pub linear_parent_uuid: Option<MessageUuid>,
    pub prompt_id: Option<delta_model::PromptId>,
    pub content: Vec<ContentBlock>,
    /// ISO-8601 timestamp from the transcript line, if present.
    pub created_at: Option<String>,
}

impl TranscriptMessage {
    /// The flattened text view of this line's content, if any.
    pub fn flatten_text(&self) -> Option<String> {
        Message::flatten_text(&self.content)
    }
}

/// Payload of a `UserPromptSubmit` hook.
#[derive(Debug, Clone)]
pub struct UserPromptSubmitHook {
    pub prompt: String,
    pub session_id: SessionId,
    pub transcript_path: String,
    pub cwd: String,
}

/// Payload of a `Stop` hook.
#[derive(Debug, Clone)]
pub struct StopHook {
    pub session_id: SessionId,
    pub stop_reason: Option<String>,
    pub last_assistant_message: Option<String>,
}

/// The fields needed to register the session on first contact.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: SessionId,
    pub cwd: String,
    pub transcript_path: String,
}

/// An event the Interactor emits for the browser to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEvent {
    /// The session was registered (first `UserPromptSubmit`).
    SessionRegistered { session_id: SessionId },
    /// A queued send was confirmed as a turn start.
    TurnStarted {
        session_id: SessionId,
        pending_send_id: i64,
        matched_uuid: MessageUuid,
    },
    /// External input was detected (typed directly into the pane).
    ExternalInput {
        session_id: SessionId,
        prompt: String,
    },
    /// A response completed.
    TurnCompleted {
        session_id: SessionId,
        stop_reason: Option<String>,
    },
    /// A tool permission prompt is imminent.
    PermissionRequested {
        session_id: SessionId,
        request_id: i64,
        tool_name: String,
    },
}

/// Drives the Claude Code session by sending keystrokes to a tmux pane.
#[async_trait]
pub trait TmuxDriver: Send + Sync {
    /// Send the given text to the target pane and submit it (Enter).
    async fn send_line(&self, text: &str) -> Result<()>;
}

/// Reads and parses the Claude Code JSONL transcript.
#[async_trait]
pub trait Transcript: Send + Sync {
    /// Read all currently available lines from the transcript at `path`,
    /// skipping the first `from_seq` lines already seen.
    async fn read_from(&self, path: &str, from_seq: usize) -> Result<Vec<TranscriptMessage>>;
}

/// Persists and queries Delta's thread overlay.
///
/// This is the irreplaceable data: thread assignment, the semantic-parent
/// graph, the send queue and permission history. Message content and linear
/// parents are a cache rebuildable from the JSONL.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Insert a session if absent and ensure its `main` thread exists, then
    /// return the session and the id of its `main` thread.
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)>;

    /// Fetch the (single) registered session, if any.
    async fn current_session(&self) -> Result<Option<Session>>;

    /// The id of a session's trunk (`main`) thread.
    async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId>;

    /// Look up a thread by id.
    async fn thread(&self, id: ThreadId) -> Result<Option<Thread>>;

    /// Create a new child thread branching off `root_message_uuid`.
    async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
        root_message_uuid: Option<&MessageUuid>,
    ) -> Result<Thread>;

    /// Enqueue a send into the FIFO and return the created row id.
    async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<PendingSend>;

    /// The oldest pending send for a session (FIFO head), if any.
    async fn head_pending_send(&self, session_id: &SessionId) -> Result<Option<PendingSend>>;

    /// Mark a pending send matched to a transcript message uuid.
    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()>;

    /// Upsert a batch of messages (content cache + overlay columns).
    async fn upsert_messages(&self, messages: &[Message]) -> Result<()>;

    /// The number of messages already stored for a session (used as the
    /// transcript read offset / next `seq`).
    async fn message_count(&self, session_id: &SessionId) -> Result<usize>;

    /// All messages for a thread, ordered by `seq`.
    async fn thread_messages(&self, thread_id: ThreadId) -> Result<Vec<Message>>;

    /// Record a tool-permission request and return the created row id.
    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
    ) -> Result<PermissionRequest>;
}
