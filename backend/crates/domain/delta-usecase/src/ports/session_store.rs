//! Persisting and querying Delta's thread overlay.

use std::collections::BTreeMap;

use async_trait::async_trait;

use delta_attribution::SubagentLaunch;
use delta_model::{
    LaunchOption, Message, MessageUuid, PermissionRequest, Send, Session, SessionId, Thread,
    ThreadId,
};

use crate::error::Result;
use crate::ports::new_session::NewSession;
use crate::session_page::SessionPageCursor;

/// One row of a session-list page: the stored session plus its `last_activity_at`
/// (the denormalized `MAX(message.created_at)`, `None` when message-less), read
/// from the session row by the page query so the usecase needs no per-row
/// follow-up lookup.
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
    ///
    /// `branch_at_launch` and `repo_root` are the spawn-time git snapshot of
    /// `cwd` — the local branch checked out and the repository root containing
    /// it. Both are `None` when the launch directory is not inside a git
    /// repository (or HEAD is detached). They are persisted once here and
    /// never updated later: see [`Session::branch_at_launch`] /
    /// [`Session::repo_root`] for the spawn-snapshot semantics.
    ///
    /// `requested_workdir` is the dir the user picked before any worktree
    /// resolution. It is `None` when no workdir was selected (the default
    /// per-token scratch dir is used). For a worktree-on spawn it holds the
    /// user-selected dir (the worktree's repo root), while `cwd` holds the
    /// auto-generated worktree path; for a plain spawn with a user-selected
    /// workdir it equals `cwd`. See [`Session::requested_workdir`].
    async fn insert_spawning_session(
        &self,
        id: &SessionId,
        cwd: &str,
        branch_at_launch: Option<&str>,
        repo_root: Option<&str>,
        requested_workdir: Option<&str>,
    ) -> Result<(Session, ThreadId)>;

    /// Delete a session row and everything it owns (threads, messages, sends,
    /// permission requests, the sync cursor — removed by cascade). Used to reap
    /// a `spawning` session whose launch failed before any data was ingested.
    async fn delete_session(&self, id: &SessionId) -> Result<()>;

    /// Mark a still-`spawning` session `failed` (its launch never bound before
    /// the deadline). A no-op for any other status, so a stale reap can never
    /// flip an already-active session.
    async fn mark_session_failed(&self, id: &SessionId) -> Result<()>;

    /// One page of sessions, ordered most-recently-active first, resuming
    /// strictly after `cursor` (or from the top when `None`).
    ///
    /// The ordering is `recency` DESC, then `created_at` DESC, then `id` DESC,
    /// where `recency = COALESCE(session.last_activity_at, session.created_at)`
    /// — read from the denormalized `last_activity_at` column, not recomputed
    /// per row, so the ordering is index-backed and the `LIMIT` bounds the work.
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

    /// The timestamp of a session's most recent message, or `None` when it has
    /// no timestamped message yet. Read from the denormalized
    /// `session.last_activity_at` column (the maintained `MAX(message.created_at)`),
    /// stored as ISO-8601 UTC, the same format messages are persisted with.
    async fn last_activity_at(&self, session_id: &SessionId) -> Result<Option<String>>;

    /// The id of a session's trunk (`main`) thread.
    async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId>;

    /// The distinct working directories sessions have run in, most-recently-used
    /// first, capped at `limit`.
    ///
    /// Derived from the `session.cwd` column (Delta keeps no separate
    /// working-directory history): one row per distinct `cwd`, ordered by the
    /// most recent activity of any session that used it. The recency key is the
    /// same one the session list uses — `COALESCE(last_activity_at, created_at)`
    /// (the denormalized last-activity column, falling back to `created_at`) —
    /// maxed across the sessions sharing that `cwd`, so a directory's place
    /// reflects when it was last actually worked in. Each row carries that
    /// recency timestamp for display.
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

    /// A single send by id, if it exists.
    ///
    /// Used to resolve the owning session of a cancel request (which carries
    /// only the send id in its URL) so the cancel can route through that
    /// session's actor, ordered against the dispatch path.
    async fn send(&self, id: i64) -> Result<Option<Send>>;

    /// The oldest still-`queued` send for a session (FIFO), if any. This is
    /// the next held-back send to dispatch when the session becomes idle.
    async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>>;

    /// A session's open (non-terminal) sends — status `queued` or
    /// `dispatched` — oldest first (ascending `id`).
    ///
    /// This is the server-side truth behind the browser's send strip:
    /// every send accepted for the session that has neither matched a
    /// transcript line nor been cancelled yet.
    async fn open_sends(&self, session_id: &SessionId) -> Result<Vec<Send>>;

    /// Promote a `queued` send to `dispatched`, marking it typed so the
    /// normal `UserPromptSubmit` correlation can match it.
    async fn promote_queued_send(&self, id: i64) -> Result<()>;

    /// Return a `dispatched` send to `queued`. A no-op for any other status.
    ///
    /// Used when the turn state machine orphans an outstanding send whose echo
    /// never arrived (see `OrphanedSend::Requeue`): the row keeps its
    /// thread/branch/quote semantics and re-dispatches when the session is
    /// next idle, so a composed message is never silently lost.
    async fn requeue_send(&self, id: i64) -> Result<()>;

    /// The outstanding dispatched send for a session, if any.
    ///
    /// Under the single-outstanding dispatch rule at most one `dispatched` row
    /// exists per session, so this *is* the send `UserPromptSubmit` correlation
    /// and transcript attribution compare against (by trimmed-text equality at
    /// the call sites). Defined as the oldest `dispatched` row so that, should
    /// the invariant ever be violated, the comparison still deterministically
    /// picks the earliest.
    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>>;

    /// Mark a dispatched send matched to a transcript message uuid.
    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()>;

    /// The thread of the latest already-persisted **user** message in a
    /// session, used as the carry-forward thread for following non-user lines.
    /// Returns `None` when the session has no user message yet.
    async fn latest_user_thread(&self, session_id: &SessionId) -> Result<Option<ThreadId>>;

    /// The thread the session's in-progress turn belongs to.
    ///
    /// Hooks that fire *inside* a turn but before its user line is ingested
    /// (`AskUserQuestion`'s `PreToolUse`, the `MessageDisplay` streaming
    /// preview) need to attribute their output to the turn's thread. They
    /// cannot read it from [`Self::latest_user_thread`] alone: a mid-turn
    /// branch send creates a brand-new thread and is dispatched without its
    /// user line yet in the JSONL (the well-known `UserPromptSubmit` timing
    /// race — the line is attributed later, at `Stop`), so `latest_user_thread`
    /// still reports the *prior* turn's thread.
    ///
    /// The authoritative source before the line lands is the in-flight send
    /// itself: the dispatched head carries the `thread_id` it was composed for.
    /// So resolution prefers, in order:
    ///
    /// 1. the [`Self::head_dispatched_send`]'s `thread_id` (the in-flight
    ///    turn's own thread, correct even before its user line is ingested),
    /// 2. [`Self::latest_user_thread`] (a turn typed straight into the pane,
    ///    with no Delta send driving it, once its line is persisted),
    /// 3. [`Self::main_thread_id`] (no send and no user line yet — a turn at
    ///    the very start of a session).
    ///
    /// Provided as a default method so the composition lives in one place and
    /// the two hook call sites cannot drift apart.
    async fn in_progress_turn_thread(&self, session_id: &SessionId) -> Result<ThreadId> {
        if let Some(send) = self.head_dispatched_send(session_id).await? {
            return Ok(send.thread_id);
        }
        if let Some(thread_id) = self.latest_user_thread(session_id).await? {
            return Ok(thread_id);
        }
        self.main_thread_id(session_id).await
    }

    /// Cancel a send by marking the row `cancelled`.
    ///
    /// The row is kept (rather than deleted) for audit, and because
    /// [`Self::head_dispatched_send`] only considers `dispatched` rows, a
    /// cancelled row no longer blocks the FIFO head. Used to roll back a
    /// `dispatched` row whose keystrokes were never delivered (e.g. a failed
    /// tmux dispatch).
    async fn cancel_send(&self, id: i64) -> Result<()>;

    /// Cancel a send **only while it is still `queued`**, returning whether a
    /// row actually transitioned.
    ///
    /// The guarded sibling of [`Self::cancel_send`]: a user-initiated cancel may
    /// only abandon a send that has not yet been typed into the pane, so the
    /// `WHERE status = 'queued'` clause makes the transition a no-op (returning
    /// `false`) the moment the send has been dispatched, matched, or already
    /// cancelled — losing a race with the idle dispatch path rather than
    /// clobbering an in-flight send. A cancelled (terminal) row drops out of
    /// [`Self::open_sends`] and is skipped by [`Self::next_queued_send`], so it
    /// is never dispatched on idle.
    async fn cancel_queued_send(&self, id: i64) -> Result<bool>;

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
    ///
    /// `tool_use_id` is `Some` for the row `PreToolUse` records for every tool
    /// call (the id later correlates the matching `tool_result`), and `None`
    /// for the row the `PermissionRequest` hook owns for a genuine interactive
    /// dialog (that hook's payload carries no tool-call id).
    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: Option<&str>,
    ) -> Result<PermissionRequest>;

    /// Decide a still-`pending` permission request from the browser, recording
    /// `allowed`/`denied` plus `decided_at`. Returns the decided row, or
    /// `None` when the row is unknown or no longer `pending` (an
    /// already-decided row is never flipped).
    async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> Result<Option<PermissionRequest>>;

    /// Resolve the open (`pending`) permission requests settled by a
    /// just-ingested `tool_result`: the `PreToolUse`-recorded row whose
    /// `tool_use_id` matches, plus any pending dialog row for the same session
    /// (`tool_use_id IS NULL` — the dialog blocks the session, so the next
    /// tool_result to arrive is the one it gated).
    ///
    /// `allowed` records the disposition inferred from the tool_result's error
    /// flag (`false` → the tool was denied). Returns the ids of the rows that
    /// transitioned (empty when there was nothing to resolve).
    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> Result<Vec<i64>>;

    /// Record (or refresh) the launching thread of a background task, keyed by
    /// the launching tool_use `id`. A background `Agent`/`Task`/`Bash`
    /// (`run_in_background: true`) returns immediately and its completion is
    /// injected later — frequently in a different sync window — as a
    /// `<task-notification>` carrying this same id. Persisting `(tool_use_id ->
    /// thread_id)` lets [`Self::outstanding_subagent_launches`] reseed the
    /// attribution fold so the notification is attributed to the launching
    /// thread rather than whatever thread is current when it lands. Idempotent:
    /// re-recording the same id is a no-op refresh (the row's `task_id`, if
    /// previously set, is preserved — see [`Self::upgrade_subagent_task_id`]).
    async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> Result<()>;

    /// Upgrade a recorded background-task launch with the `task_id` Claude Code
    /// minted for the subagent. The id is learned later than the launch itself:
    /// the `PostToolUse(Agent)` hook reads it from the launching tool's
    /// `tool_result` (the `agentId` field) and persists it here so a subsequent
    /// `<task-notification>` whose `<tool-use-id>` element was stripped can
    /// still be matched by its `<task-id>` element. Idempotent: re-upgrading
    /// the same id with the same `task_id` is a no-op refresh. Upgrading an
    /// unknown launch is also a no-op (the launch may have been folded already).
    async fn upgrade_subagent_task_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        task_id: &str,
    ) -> Result<()>;

    /// Clear a recorded background-task launch once its `<task-notification>`
    /// has been folded. Clearing an unknown id is a no-op.
    async fn clear_subagent_launch(&self, session_id: &SessionId, tool_use_id: &str)
        -> Result<()>;

    /// The session's outstanding background-task launches as `(tool_use_id ->
    /// SubagentLaunch)`: every background task still awaiting its
    /// `<task-notification>`, each entry pairing the launching thread with the
    /// optional `task_id` learned via `PostToolUse(Agent)`. Seeds the
    /// attribution fold at sync start so a completion landing in a later
    /// window finds its launching thread by either correlation key.
    async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<BTreeMap<String, SubagentLaunch>>;

    /// All registered launch options, newest first (descending `id`).
    async fn list_launch_options(&self) -> Result<Vec<LaunchOption>>;

    /// Register a launch option and return the created row. `label` and `value`
    /// are optional (a valueless flag carries no `value`); `name` is the flag.
    /// `default_enabled` marks it to start pre-checked in the session-start
    /// picker.
    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
    ) -> Result<LaunchOption>;

    /// Set a launch option's `default_enabled` flag, returning the updated row,
    /// or `None` if no option has that id.
    async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> Result<Option<LaunchOption>>;

    /// Delete a launch option by id. Deleting an unknown id is a no-op.
    async fn delete_launch_option(&self, id: i64) -> Result<()>;
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
        branch_at_launch: Option<&str>,
        repo_root: Option<&str>,
        requested_workdir: Option<&str>,
    ) -> Result<(Session, ThreadId)> {
        (**self)
            .insert_spawning_session(id, cwd, branch_at_launch, repo_root, requested_workdir)
            .await
    }

    async fn delete_session(&self, id: &SessionId) -> Result<()> {
        (**self).delete_session(id).await
    }

    async fn mark_session_failed(&self, id: &SessionId) -> Result<()> {
        (**self).mark_session_failed(id).await
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

    async fn send(&self, id: i64) -> Result<Option<Send>> {
        (**self).send(id).await
    }

    async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        (**self).next_queued_send(session_id).await
    }

    async fn open_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        (**self).open_sends(session_id).await
    }

    async fn promote_queued_send(&self, id: i64) -> Result<()> {
        (**self).promote_queued_send(id).await
    }

    async fn requeue_send(&self, id: i64) -> Result<()> {
        (**self).requeue_send(id).await
    }

    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        (**self).head_dispatched_send(session_id).await
    }

    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()> {
        (**self).mark_send_matched(id, matched_uuid).await
    }

    async fn latest_user_thread(&self, session_id: &SessionId) -> Result<Option<ThreadId>> {
        (**self).latest_user_thread(session_id).await
    }

    async fn cancel_send(&self, id: i64) -> Result<()> {
        (**self).cancel_send(id).await
    }

    async fn cancel_queued_send(&self, id: i64) -> Result<bool> {
        (**self).cancel_queued_send(id).await
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
        tool_use_id: Option<&str>,
    ) -> Result<PermissionRequest> {
        (**self)
            .record_permission_request(session_id, tool_name, tool_input_json, tool_use_id)
            .await
    }

    async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> Result<Option<PermissionRequest>> {
        (**self).decide_permission_request(request_id, allowed).await
    }

    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> Result<Vec<i64>> {
        (**self)
            .resolve_permission_by_tool_use_id(session_id, tool_use_id, allowed)
            .await
    }

    async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> Result<()> {
        (**self)
            .record_subagent_launch(session_id, tool_use_id, thread_id)
            .await
    }

    async fn upgrade_subagent_task_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        task_id: &str,
    ) -> Result<()> {
        (**self)
            .upgrade_subagent_task_id(session_id, tool_use_id, task_id)
            .await
    }

    async fn clear_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
    ) -> Result<()> {
        (**self)
            .clear_subagent_launch(session_id, tool_use_id)
            .await
    }

    async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<BTreeMap<String, SubagentLaunch>> {
        (**self).outstanding_subagent_launches(session_id).await
    }

    async fn list_launch_options(&self) -> Result<Vec<LaunchOption>> {
        (**self).list_launch_options().await
    }

    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
    ) -> Result<LaunchOption> {
        (**self)
            .create_launch_option(label, name, value, default_enabled)
            .await
    }

    async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> Result<Option<LaunchOption>> {
        (**self)
            .set_launch_option_default_enabled(id, default_enabled)
            .await
    }

    async fn delete_launch_option(&self, id: i64) -> Result<()> {
        (**self).delete_launch_option(id).await
    }
}
