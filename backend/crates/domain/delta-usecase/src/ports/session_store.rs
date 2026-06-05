//! Persisting and querying Delta's thread overlay.

use async_trait::async_trait;

use delta_model::{Message, MessageUuid, PendingSend, PermissionRequest, Session, SessionId, Thread, ThreadId};

use crate::error::Result;
use crate::ports::new_session::NewSession;

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

    /// All threads for a session, ordered by creation (ascending `id`).
    async fn list_threads(&self, session_id: &SessionId) -> Result<Vec<Thread>>;

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

#[async_trait]
impl SessionStore for Box<dyn SessionStore> {
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
        (**self).register_session(new).await
    }

    async fn current_session(&self) -> Result<Option<Session>> {
        (**self).current_session().await
    }

    async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId> {
        (**self).main_thread_id(session_id).await
    }

    async fn thread(&self, id: ThreadId) -> Result<Option<Thread>> {
        (**self).thread(id).await
    }

    async fn list_threads(&self, session_id: &SessionId) -> Result<Vec<Thread>> {
        (**self).list_threads(session_id).await
    }

    async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
        root_message_uuid: Option<&MessageUuid>,
    ) -> Result<Thread> {
        (**self)
            .create_thread(session_id, title, parent_thread_id, root_message_uuid)
            .await
    }

    async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<PendingSend> {
        (**self)
            .enqueue_send(
                session_id,
                thread_id,
                semantic_parent_uuid,
                text,
                locator_quote,
            )
            .await
    }

    async fn head_pending_send(&self, session_id: &SessionId) -> Result<Option<PendingSend>> {
        (**self).head_pending_send(session_id).await
    }

    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()> {
        (**self).mark_send_matched(id, matched_uuid).await
    }

    async fn upsert_messages(&self, messages: &[Message]) -> Result<()> {
        (**self).upsert_messages(messages).await
    }

    async fn message_count(&self, session_id: &SessionId) -> Result<usize> {
        (**self).message_count(session_id).await
    }

    async fn thread_messages(&self, thread_id: ThreadId) -> Result<Vec<Message>> {
        (**self).thread_messages(thread_id).await
    }

    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
    ) -> Result<PermissionRequest> {
        (**self)
            .record_permission_request(session_id, tool_name, tool_input_json)
            .await
    }
}
