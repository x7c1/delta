//! Persisting and querying Delta's thread overlay.

use async_trait::async_trait;

use delta_model::{
    Message, MessageUuid, PermissionRequest, Send, Session, SessionId, Thread, ThreadId,
};

use crate::error::Result;
use crate::ports::new_session::NewSession;
use crate::session_page::SessionPageCursor;

/// One row of a session-list page: the stored session plus its `last_activity_at`
/// (`MAX(message.created_at)`, `None` when message-less), fetched inline by the
/// page query so the usecase needs no per-row follow-up lookup.
pub type SessionPageRow = (Session, Option<String>);

/// One recently-used working directory: its absolute `cwd` and the timestamp of
/// the latest activity in any session that used it (`MAX(message.created_at)`,
/// falling back to the session's `created_at`; `None` only when a contributing
/// session is itself message-less and has a null `created_at`, which does not
/// occur in practice). Derived from the `session.cwd` column — Delta keeps no
/// separate working-directory history.
pub type RecentWorkdir = (String, Option<String>);

/// Persists and queries Delta's thread overlay.
///
/// This is the irreplaceable data: thread assignment, the semantic-parent
/// graph, the send queue and permission history. Message content and linear
/// parents are a cache rebuildable from the JSONL.
// `delta_model::Send` (imported above) shadows the `std::marker::Send` prelude
// trait in this module, so the auto-trait bound is spelled out explicitly.
#[async_trait]
pub trait SessionStore: std::marker::Send + Sync {
    /// Insert a session if absent and ensure its `main` thread exists, then
    /// return the session and the id of its `main` thread.
    ///
    /// When the row already exists as a Delta-launched `spawning` session
    /// (written by [`Self::insert_spawning_session`] when the id was minted),
    /// this first hook contact *activates* it: the status flips to `active` and
    /// the hook-reported `transcript_path` (unknown at mint time) is filled in.
    /// An already-active/ended row is left untouched.
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)>;

    /// Insert a brand-new `spawning` session row plus its `main` thread, before
    /// `claude` is launched. The transcript path is `NULL` (it is owned by
    /// Claude Code and only learned from the first hook, which activates the
    /// row via [`Self::register_session`]). The id is freshly minted, so an
    /// existing row with the same id is an error, not an upsert.
    async fn insert_spawning_session(
        &self,
        id: &SessionId,
        cwd: &str,
    ) -> Result<(Session, ThreadId)>;

    /// Delete a session row and everything it owns (threads, messages, sends,
    /// permission requests, the sync cursor — removed by cascade). Used to reap
    /// a `spawning` session whose launch failed before any data was ingested.
    async fn delete_session(&self, id: &SessionId) -> Result<()>;

    /// Mark a still-`spawning` session `failed` (its launch never bound before
    /// the deadline). A no-op for any other status, so a stale reap can never
    /// flip an already-active session.
    async fn mark_session_failed(&self, id: &SessionId) -> Result<()>;

    /// All registered sessions, ordered by creation (ascending `created_at`).
    async fn list_sessions(&self) -> Result<Vec<Session>>;

    /// One page of sessions, ordered most-recently-active first, resuming
    /// strictly after `cursor` (or from the top when `None`).
    ///
    /// The ordering is `recency` DESC, then `created_at` DESC, then `id` ASC,
    /// where `recency = COALESCE(MAX(message.created_at), session.created_at)`.
    /// Each row carries its raw `last_activity_at` (`None` when message-less) so
    /// the usecase needs no per-row activity lookup. At most `limit` rows are
    /// returned; a full page (`len == limit`) signals more may follow.
    async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<Vec<SessionPageRow>>;

    /// Look up a session by id, if it exists.
    async fn session(&self, id: &SessionId) -> Result<Option<Session>>;

    /// The timestamp of a session's most recent message
    /// (`MAX(message.created_at)`), or `None` when it has no messages yet.
    /// Stored as ISO-8601 UTC, the same format messages are persisted with.
    async fn last_activity_at(&self, session_id: &SessionId) -> Result<Option<String>>;

    /// The id of a session's trunk (`main`) thread.
    async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId>;

    /// The distinct working directories sessions have run in, most-recently-used
    /// first, capped at `limit`.
    ///
    /// Derived from the `session.cwd` column (Delta keeps no separate
    /// working-directory history): one row per distinct `cwd`, ordered by the
    /// most recent activity of any session that used it. The recency key is the
    /// same one the session list uses — `MAX(message.created_at)` over the
    /// sessions sharing that `cwd`, falling back to their `created_at` — so a
    /// directory's place reflects when it was last actually worked in. Each row
    /// carries that recency timestamp for display.
    async fn recent_workdirs(&self, limit: u32) -> Result<Vec<RecentWorkdir>>;

    /// Look up a thread by id.
    async fn thread(&self, id: ThreadId) -> Result<Option<Thread>>;

    /// All threads for a session, ordered by creation (ascending `id`).
    async fn list_threads(&self, session_id: &SessionId) -> Result<Vec<Thread>>;

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

    /// Record a send in the `dispatched` state (its keystrokes are about to be
    /// typed into the pane) and return the created row.
    async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send>;

    /// Record a send held in the `queued` state: its row is recorded (so the
    /// branch thread and the queued text persist) but its keystrokes are not
    /// dispatched yet. Delta promotes it with [`Self::promote_queued_send`]
    /// and dispatches it once the session goes idle.
    async fn enqueue_queued_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send>;

    /// The oldest still-`queued` send for a session (FIFO), if any. This is
    /// the next held-back send to dispatch when the session becomes idle.
    async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>>;

    /// Promote a `queued` send to `dispatched`, marking it typed so the
    /// normal `UserPromptSubmit` correlation can match it.
    async fn promote_queued_send(&self, id: i64) -> Result<()>;

    /// Whether a turn is currently in flight for this session. Set when Delta
    /// dispatches a send and when a `UserPromptSubmit` arrives (so turns typed
    /// straight into the pane are tracked too), and cleared when the turn
    /// completes (`Stop`) or is interrupted. A branch/quoted send issued while
    /// this is set is queued rather than dispatched mid-turn.
    async fn is_turn_active(&self, session_id: &SessionId) -> Result<bool>;

    /// Set the in-flight-turn flag for a session.
    async fn set_turn_active(&self, session_id: &SessionId, active: bool) -> Result<()>;

    /// The oldest dispatched send for a session (FIFO head), if any.
    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>>;

    /// Mark a dispatched send matched to a transcript message uuid.
    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()>;

    /// Find the oldest still-`dispatched` send for a session whose trimmed text
    /// equals `trimmed_text`, if any.
    ///
    /// Drives thread attribution during ingestion: a user transcript line is
    /// matched to its queued send by text, so it is attributed to that send's
    /// thread regardless of which hook triggered the sync or whether the line
    /// was present when `UserPromptSubmit` fired. `trimmed_text` is expected to
    /// be already trimmed by the caller.
    async fn match_dispatched_send(
        &self,
        session_id: &SessionId,
        trimmed_text: &str,
    ) -> Result<Option<Send>>;

    /// The thread of the latest already-persisted **user** message in a
    /// session, used as the carry-forward thread for following non-user lines.
    /// Returns `None` when the session has no user message yet.
    async fn latest_user_thread(&self, session_id: &SessionId) -> Result<Option<ThreadId>>;

    /// Cancel a send by marking the row `cancelled`.
    ///
    /// The row is kept (rather than deleted) for audit, and because
    /// [`Self::head_dispatched_send`] only considers `dispatched` rows, a
    /// cancelled row no longer blocks the FIFO head. Used to roll back a
    /// `dispatched` row whose keystrokes were never delivered (e.g. a failed
    /// tmux dispatch).
    async fn cancel_send(&self, id: i64) -> Result<()>;

    /// Upsert a batch of messages (content cache + overlay columns).
    async fn upsert_messages(&self, messages: &[Message]) -> Result<()>;

    /// The number of messages already stored for a session.
    async fn message_count(&self, session_id: &SessionId) -> Result<usize>;

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

    /// Record a tool-permission request and return the created row.
    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: &str,
    ) -> Result<PermissionRequest>;

    /// Find the open (`pending`) permission request that a `PermissionRequest`
    /// hook refers to. The hook carries no `tool_use_id`, so match by
    /// (session, tool_name) and prefer an exact `tool_input_json` match, falling
    /// back to the most recent pending request for that tool. Returns its id.
    async fn find_open_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
    ) -> Result<Option<i64>>;

    /// Resolve the open (`pending`) permission request for this session whose
    /// `tool_use_id` matches the just-ingested `tool_result`, if any.
    ///
    /// `allowed` records the disposition inferred from the tool_result's error
    /// flag (`false` → the tool was denied). Returns the resolved request's id
    /// when a pending request matched, or `None` when there was nothing to
    /// resolve (already decided, or no such request).
    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> Result<Option<i64>>;
}

#[async_trait]
impl SessionStore for Box<dyn SessionStore> {
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
        (**self).register_session(new).await
    }

    async fn insert_spawning_session(
        &self,
        id: &SessionId,
        cwd: &str,
    ) -> Result<(Session, ThreadId)> {
        (**self).insert_spawning_session(id, cwd).await
    }

    async fn delete_session(&self, id: &SessionId) -> Result<()> {
        (**self).delete_session(id).await
    }

    async fn mark_session_failed(&self, id: &SessionId) -> Result<()> {
        (**self).mark_session_failed(id).await
    }

    async fn list_sessions(&self) -> Result<Vec<Session>> {
        (**self).list_sessions().await
    }

    async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<Vec<SessionPageRow>> {
        (**self).list_sessions_page(cursor, limit).await
    }

    async fn session(&self, id: &SessionId) -> Result<Option<Session>> {
        (**self).session(id).await
    }

    async fn last_activity_at(&self, session_id: &SessionId) -> Result<Option<String>> {
        (**self).last_activity_at(session_id).await
    }

    async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId> {
        (**self).main_thread_id(session_id).await
    }

    async fn recent_workdirs(&self, limit: u32) -> Result<Vec<RecentWorkdir>> {
        (**self).recent_workdirs(limit).await
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
    ) -> Result<Thread> {
        (**self)
            .create_thread(session_id, title, parent_thread_id)
            .await
    }

    async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
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

    async fn enqueue_queued_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        (**self)
            .enqueue_queued_send(
                session_id,
                thread_id,
                semantic_parent_uuid,
                text,
                locator_quote,
            )
            .await
    }

    async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        (**self).next_queued_send(session_id).await
    }

    async fn promote_queued_send(&self, id: i64) -> Result<()> {
        (**self).promote_queued_send(id).await
    }

    async fn is_turn_active(&self, session_id: &SessionId) -> Result<bool> {
        (**self).is_turn_active(session_id).await
    }

    async fn set_turn_active(&self, session_id: &SessionId, active: bool) -> Result<()> {
        (**self).set_turn_active(session_id, active).await
    }

    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        (**self).head_dispatched_send(session_id).await
    }

    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()> {
        (**self).mark_send_matched(id, matched_uuid).await
    }

    async fn match_dispatched_send(
        &self,
        session_id: &SessionId,
        trimmed_text: &str,
    ) -> Result<Option<Send>> {
        (**self).match_dispatched_send(session_id, trimmed_text).await
    }

    async fn latest_user_thread(&self, session_id: &SessionId) -> Result<Option<ThreadId>> {
        (**self).latest_user_thread(session_id).await
    }

    async fn cancel_send(&self, id: i64) -> Result<()> {
        (**self).cancel_send(id).await
    }

    async fn upsert_messages(&self, messages: &[Message]) -> Result<()> {
        (**self).upsert_messages(messages).await
    }

    async fn message_count(&self, session_id: &SessionId) -> Result<usize> {
        (**self).message_count(session_id).await
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

    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: &str,
    ) -> Result<PermissionRequest> {
        (**self)
            .record_permission_request(session_id, tool_name, tool_input_json, tool_use_id)
            .await
    }

    async fn find_open_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
    ) -> Result<Option<i64>> {
        (**self)
            .find_open_permission_request(session_id, tool_name, tool_input_json)
            .await
    }

    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> Result<Option<i64>> {
        (**self)
            .resolve_permission_by_tool_use_id(session_id, tool_use_id, allowed)
            .await
    }
}
