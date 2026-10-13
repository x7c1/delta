//! The message cache: transcript upserts, the ingestion cursor, and reads.

use async_trait::async_trait;

use delta_model::{Message, MessageUuid, SessionId, ThreadId};

use super::SessionStore;
use crate::error::Result;

/// The message cache rebuilt from the transcript, plus the line-based
/// ingestion cursor that feeds it.
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait MessageStore: Send + Sync {
    /// Upsert a batch of messages (content cache + overlay columns).
    async fn upsert_messages(&self, messages: &[Message]) -> Result<()>;

    /// The number of messages already stored for a session.
    async fn message_count(&self, session_id: &SessionId) -> Result<usize>;

    /// The uuid of the session's message with the highest `seq` — for a Claude
    /// session, the transcript line ingested furthest down the file — or `None`
    /// when nothing has been ingested yet.
    async fn latest_message_uuid(&self, session_id: &SessionId) -> Result<Option<MessageUuid>>;

    /// The number of transcript lines already consumed for a session: the
    /// line-based ingestion cursor. The next read starts at this index, so each
    /// transcript line is processed exactly once regardless of how many of them
    /// parsed into messages.
    async fn transcript_lines_read(&self, session_id: &SessionId) -> Result<usize>;

    /// Advance the line-based ingestion cursor to `lines` (the transcript's
    /// total line count after the latest read).
    async fn set_transcript_lines_read(&self, session_id: &SessionId, lines: usize) -> Result<()>;

    /// All messages for a thread, ordered by `seq`.
    async fn thread_messages(&self, thread_id: ThreadId) -> Result<Vec<Message>>;

    /// The timestamp of a session's most recent message, or `None` when it has
    /// no timestamped message yet. Read from the denormalized
    /// `session.last_activity_at` column (the maintained `MAX(message.created_at)`),
    /// stored as ISO-8601 UTC, the same format messages are persisted with.
    async fn last_activity_at(&self, session_id: &SessionId) -> Result<Option<String>>;
}

#[async_trait]
impl MessageStore for Box<dyn SessionStore> {
    async fn upsert_messages(&self, messages: &[Message]) -> Result<()> {
        (**self).upsert_messages(messages).await
    }

    async fn message_count(&self, session_id: &SessionId) -> Result<usize> {
        (**self).message_count(session_id).await
    }

    async fn latest_message_uuid(&self, session_id: &SessionId) -> Result<Option<MessageUuid>> {
        (**self).latest_message_uuid(session_id).await
    }

    async fn transcript_lines_read(&self, session_id: &SessionId) -> Result<usize> {
        (**self).transcript_lines_read(session_id).await
    }

    async fn set_transcript_lines_read(&self, session_id: &SessionId, lines: usize) -> Result<()> {
        (**self).set_transcript_lines_read(session_id, lines).await
    }

    async fn thread_messages(&self, thread_id: ThreadId) -> Result<Vec<Message>> {
        (**self).thread_messages(thread_id).await
    }

    async fn last_activity_at(&self, session_id: &SessionId) -> Result<Option<String>> {
        (**self).last_activity_at(session_id).await
    }
}
