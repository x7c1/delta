//! Thread rows: the trunk, branch threads, and thread lookups.

use async_trait::async_trait;

use delta_model::{SessionId, Thread, ThreadId};

use super::SessionStore;
use crate::error::Result;

/// The thread rows: each session's trunk, its branch threads, and the
/// thread a session's latest user message belongs to.
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait ThreadStore: Send + Sync {
    /// The id of a session's trunk (`main`) thread.
    async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId>;

    /// Look up a thread by id.
    async fn thread(&self, id: ThreadId) -> Result<Option<Thread>>;

    /// All threads for a session, ordered by creation (ascending `id`).
    async fn list_threads(&self, session_id: &SessionId) -> Result<Vec<Thread>>;

    /// All threads of several sessions in one query, ordered by creation
    /// (ascending `id`) — so each session's threads, taken in result order,
    /// come out in the order [`Self::list_threads`] returns them.
    ///
    /// Backs the session list, which carries every listed session's threads:
    /// one query per page rather than one per row. An id with no session (or no
    /// threads) contributes nothing. An empty `session_ids` returns an empty
    /// `Vec` without touching the database.
    async fn list_threads_by_session_ids(&self, session_ids: &[SessionId]) -> Result<Vec<Thread>>;

    /// Create a new child thread under `parent_thread_id`.
    ///
    /// The message the thread branches from is NOT passed here: the branch edge
    /// lives on the thread's first send/message as `semantic_parent_uuid`, and
    /// [`Thread::root_message_uuid`] is derived from it on read.
    async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
    ) -> Result<Thread>;

    /// The thread of the latest already-persisted **user** message in a
    /// session, used as the carry-forward thread for following non-user lines.
    /// Returns `None` when the session has no user message yet.
    async fn latest_user_thread(&self, session_id: &SessionId) -> Result<Option<ThreadId>>;
}

#[async_trait]
impl ThreadStore for Box<dyn SessionStore> {
    async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId> {
        (**self).main_thread_id(session_id).await
    }

    async fn thread(&self, id: ThreadId) -> Result<Option<Thread>> {
        (**self).thread(id).await
    }

    async fn list_threads(&self, session_id: &SessionId) -> Result<Vec<Thread>> {
        (**self).list_threads(session_id).await
    }

    async fn list_threads_by_session_ids(&self, session_ids: &[SessionId]) -> Result<Vec<Thread>> {
        (**self).list_threads_by_session_ids(session_ids).await
    }

    async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
    ) -> Result<Thread> {
        (**self)
            .create_thread(session_id, title, parent_thread_id)
            .await
    }

    async fn latest_user_thread(&self, session_id: &SessionId) -> Result<Option<ThreadId>> {
        (**self).latest_user_thread(session_id).await
    }
}
