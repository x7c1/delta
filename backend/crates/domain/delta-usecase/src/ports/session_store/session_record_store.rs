//! Session rows: registration, the spawn lifecycle, and the recency-ordered
//! session list.

use async_trait::async_trait;

use delta_model::{Session, SessionId, SessionStatus, ThreadId};

use super::session_page_row::SessionPageRow;
use super::SessionStore;
use crate::error::Result;
use crate::ports::new_session::NewSession;
use crate::ports::spawning_session::SpawningSession;
use crate::session_page_cursor::SessionPageCursor;

/// The session rows: registration, the spawn lifecycle, removal, and the
/// recency-ordered pages of the session list.
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait SessionRecordStore: Send + Sync {
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
    ///
    /// The [`SpawningSession`] fields document each column of the snapshot.
    async fn insert_spawning_session(
        &self,
        spawning: SpawningSession<'_>,
    ) -> Result<(Session, ThreadId)>;

    /// Record the provider-minted conversation identifiers for a session: the
    /// `provider_session_id` and `provider_thread_id` a structured provider
    /// (e.g. Codex) returns from its launch/`thread/start` call. Overwrites
    /// whatever was stored (both start `NULL` at spawn). A Claude session never
    /// calls this — its conversation id is the Delta-minted [`SessionId`], so
    /// both columns stay `NULL`.
    ///
    /// If the row is still [`SessionStatus::Spawning`] this also activates it
    /// (→ [`SessionStatus::Active`]): a terminal-less structured provider has no
    /// hook to flip the status the way Claude's first `UserPromptSubmit` does
    /// (via [`Self::register_session`]), so the launch-return that yields these
    /// ids is the moment the session is confirmed live. An already-active or
    /// ended row keeps its status.
    ///
    /// [`SessionStatus::Spawning`]: delta_model::SessionStatus::Spawning
    /// [`SessionStatus::Active`]: delta_model::SessionStatus::Active
    async fn set_provider_ids(
        &self,
        id: &SessionId,
        provider_session_id: Option<&str>,
        provider_thread_id: Option<&str>,
    ) -> Result<()>;

    /// Delete a session row and everything it owns (threads, messages, sends,
    /// permission requests, the sync cursor — removed by cascade). This is the
    /// user removing a session from Delta's list; a launch that failed is
    /// marked, not deleted (see [`Self::mark_session_failed`]).
    async fn delete_session(&self, id: &SessionId) -> Result<()>;

    /// Mark a still-`spawning` session `failed` — its launch ended without ever
    /// binding — and record `reason`, the text that says why, when Delta could
    /// name one. A no-op for any other status, so a stale reap can never flip an
    /// already-active session.
    ///
    /// The row and everything hanging off it are KEPT — see
    /// [`SessionStatus::Failed`].
    async fn mark_session_failed(&self, id: &SessionId, reason: Option<&str>) -> Result<()>;

    /// Record why Delta closed a session on its own, for the closed session's
    /// screen ([`Session::closed_reason`]). A missing row is a no-op.
    ///
    /// The reason stays until the session is bound to a pane again:
    /// [`RememberedPaneStore::remember_pane`] clears it in the same write.
    ///
    /// [`Session::closed_reason`]: delta_model::Session::closed_reason
    /// [`RememberedPaneStore::remember_pane`]: super::RememberedPaneStore::remember_pane
    async fn record_closed_reason(&self, id: &SessionId, reason: &str) -> Result<()>;

    /// Point a session at the transcript Claude Code moved it to, in any
    /// status.
    ///
    /// Claude Code moves a session's JSONL to the project directory of its new
    /// working directory when the session enters a worktree, and reports the
    /// new path in every later hook. [`Self::register_session`] only fills the
    /// path in while the row is `spawning`, so this is the write that follows
    /// such a move. `cwd`, when `Some`, replaces the session's working
    /// directory too (the move's `relocated` line names it), so a later
    /// `claude --resume` runs where Claude Code now files the transcript; `None`
    /// leaves it as it was. A missing row is a no-op.
    async fn relocate_transcript(
        &self,
        id: &SessionId,
        transcript_path: &str,
        cwd: Option<&str>,
    ) -> Result<()>;

    /// One page of sessions in the store's recency stream, resuming strictly
    /// after `cursor` (or from the top when `None`).
    ///
    /// The ordering is `recency` DESC, then `created_at` DESC, then `id` DESC,
    /// where `recency = COALESCE(session.last_activity_at, session.created_at)`
    /// — read from the denormalized `last_activity_at` column, not recomputed
    /// per row, so the ordering is index-backed and the `LIMIT` bounds the work.
    /// Each row carries its raw `last_activity_at` (`None` when message-less) so
    /// the usecase needs no per-row activity lookup. At most `limit` rows are
    /// returned; a full page (`len == limit`) signals more may follow.
    ///
    /// This is pure recency: liveness is process-runtime state, not a column,
    /// so the store cannot sort on it. The API's open-first ordering is layered
    /// on top by [`Interactor::list_sessions_page`], which pulls the live
    /// sessions' rows through [`Self::list_sessions_by_ids`] and filters them
    /// out of this stream.
    ///
    /// [`Interactor::list_sessions_page`]: crate::Interactor::list_sessions_page
    async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<Vec<SessionPageRow>>;

    /// The rows of specific sessions, in the session list's recency order.
    ///
    /// Same shape and ordering key as [`Self::list_sessions_page`] (`recency`
    /// DESC, `created_at` DESC, `id` DESC, where
    /// `recency = COALESCE(last_activity_at, created_at)`), but selected by id
    /// rather than paged: it backs the open-first head of the session list,
    /// where the set of live sessions is known to the usecase and bounded by the
    /// number of live panes.
    ///
    /// An id with no row is silently skipped (an accepted spawn whose row was
    /// reaped between the liveness snapshot and this query), so the result may
    /// be shorter than `ids`. An empty `ids` returns an empty `Vec` without
    /// touching the database.
    async fn list_sessions_by_ids(&self, ids: &[SessionId]) -> Result<Vec<SessionPageRow>>;

    /// Look up a session by id, if it exists.
    async fn session(&self, id: &SessionId) -> Result<Option<Session>>;

    /// The ids of the sessions whose most recent activity is at or before
    /// `cutoff` and whose row status is one of `statuses`, oldest first.
    ///
    /// "Most recent activity" is the session list's recency key,
    /// `COALESCE(last_activity_at, created_at)`; `cutoff` is an ISO-8601 UTC
    /// timestamp in the form [`delta_model::iso8601_utc`] writes, so the two
    /// compare as text. An empty `statuses` matches nothing.
    ///
    /// Backs the bulk removal of old sessions. The row status is all this
    /// knows: whether a matching session is open or still starting is
    /// process-runtime state, which the caller checks before removing any.
    async fn list_prunable_sessions(
        &self,
        cutoff: &str,
        statuses: &[SessionStatus],
    ) -> Result<Vec<SessionId>>;
}

#[async_trait]
impl SessionRecordStore for Box<dyn SessionStore> {
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
        (**self).register_session(new).await
    }

    async fn insert_spawning_session(
        &self,
        spawning: SpawningSession<'_>,
    ) -> Result<(Session, ThreadId)> {
        (**self).insert_spawning_session(spawning).await
    }

    async fn set_provider_ids(
        &self,
        id: &SessionId,
        provider_session_id: Option<&str>,
        provider_thread_id: Option<&str>,
    ) -> Result<()> {
        (**self)
            .set_provider_ids(id, provider_session_id, provider_thread_id)
            .await
    }

    async fn delete_session(&self, id: &SessionId) -> Result<()> {
        (**self).delete_session(id).await
    }

    async fn mark_session_failed(&self, id: &SessionId, reason: Option<&str>) -> Result<()> {
        (**self).mark_session_failed(id, reason).await
    }

    async fn record_closed_reason(&self, id: &SessionId, reason: &str) -> Result<()> {
        (**self).record_closed_reason(id, reason).await
    }

    async fn relocate_transcript(
        &self,
        id: &SessionId,
        transcript_path: &str,
        cwd: Option<&str>,
    ) -> Result<()> {
        (**self).relocate_transcript(id, transcript_path, cwd).await
    }

    async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<Vec<SessionPageRow>> {
        (**self).list_sessions_page(cursor, limit).await
    }

    async fn list_sessions_by_ids(&self, ids: &[SessionId]) -> Result<Vec<SessionPageRow>> {
        (**self).list_sessions_by_ids(ids).await
    }

    async fn session(&self, id: &SessionId) -> Result<Option<Session>> {
        (**self).session(id).await
    }

    async fn list_prunable_sessions(
        &self,
        cutoff: &str,
        statuses: &[SessionStatus],
    ) -> Result<Vec<SessionId>> {
        (**self).list_prunable_sessions(cutoff, statuses).await
    }
}
