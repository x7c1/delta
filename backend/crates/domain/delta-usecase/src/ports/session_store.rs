//! Persisting and querying Delta's thread overlay.

use std::collections::BTreeMap;

use async_trait::async_trait;

use delta_attribution::SubagentLaunch;
use delta_model::{
    AgentProvider, LaunchOption, Message, MessageUuid, PermissionRequest, PromptTemplate, Send,
    Session, SessionId, Thread, ThreadId,
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

/// One row of the Repository tab's aggregation: a single `(repo_root, clone)`
/// pair drawn from the session history, carrying enough state for the
/// interactor to bundle clones into repositories.
///
/// - `repo_root`: the repository root the clone lives under. Sessions launched
///   outside any git repo (no `session.repo_root`) never contribute — the
///   Repository tab is by definition a list of git repositories.
/// - `clone_path`: the dir the user picked at spawn time
///   (`session.requested_workdir`, falling back to `session.cwd` for sessions
///   that predate that column).
/// - `last_opened_at`: the most recent activity at this `(repo_root, clone)`
///   pair — `MAX(COALESCE(last_activity_at, created_at))` over its sessions.
/// - `last_branch`: the `branch_at_launch` of the latest session at this pair,
///   when one was recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryCloneRow {
    pub repo_root: String,
    pub clone_path: String,
    pub last_opened_at: Option<String>,
    pub last_branch: Option<String>,
}

/// One registered clone root: a directory where the user's git clones live,
/// whose direct children the Repository tab probes for clones on every list
/// call. Session-independent (no foreign key, never cascaded), so a clone root
/// outlives any individual session and is only ever rewritten through the
/// dedicated CRUD endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneRoot {
    pub path: String,
    pub created_at: String,
}

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
    /// `branch_at_launch` and `repo_root` are the spawn-time git snapshot —
    /// the local branch checked out at `cwd`, and the repository root the
    /// spawn resolved against the dir the user picked: for a worktree spawn
    /// that is the repository the worktree was cut from, which does not
    /// contain `cwd`. Both are `None` when the launch directory is not inside a git
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
    ///
    /// `repository_display_name` is the cross-worktree repository identity
    /// label (`org/repo` from the `origin` URL, or the working-tree basename
    /// when no origin is set). It is `None` when the launch directory is not
    /// inside a git repository. Persisted once here and never updated later:
    /// see [`Session::repository_display_name`] for the spawn-snapshot
    /// semantics.
    ///
    /// `provider` is the AI-agent backend the session runs on, recorded in the
    /// `session.provider` column. Every Claude spawn passes
    /// [`AgentProvider::Claude`] (the historical default); a structured
    /// provider such as Codex passes its own value. The provider-minted
    /// conversation ids are not known yet at spawn time — they are learned from
    /// the provider's launch response and written later via
    /// [`Self::set_provider_ids`].
    // Each parameter is a distinct spawn-time column; bundling them into a
    // struct would not clarify the call sites (mirrors `Interactor::new`).
    #[allow(clippy::too_many_arguments)]
    async fn insert_spawning_session(
        &self,
        id: &SessionId,
        cwd: &str,
        branch_at_launch: Option<&str>,
        repo_root: Option<&str>,
        requested_workdir: Option<&str>,
        repository_display_name: Option<&str>,
        provider: AgentProvider,
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

    /// Whether `path` is a known working directory of some session or message.
    ///
    /// Backs the `open cwd` allowlist: the REST endpoint only spawns an
    /// external tool against a path Delta has actually shown the browser, so
    /// a hand-crafted request cannot point the editor at an arbitrary path on
    /// disk. The check is a set membership over `session.cwd`,
    /// `session.requested_workdir`, and `message.cwd` — the same three columns
    /// the UI surfaces cwd values from.
    ///
    /// The path is compared verbatim; canonicalisation is the caller's job
    /// (the browser always sends the same string the server sent it, so no
    /// normalisation is needed at this layer).
    async fn cwd_exists(&self, path: &str) -> Result<bool>;

    /// One row per `(repo_root, clone_path)` pair in the session history, for
    /// the Repository tab.
    ///
    /// Drawn from `session` rows with a non-null `repo_root` (sessions outside
    /// any git repo do not contribute); the clone path is
    /// `COALESCE(requested_workdir, cwd)` so worktree-managed cwds do not leak
    /// in. Each row carries the most recent activity at that pair (the
    /// `MAX(COALESCE(last_activity_at, created_at))` of its sessions) and the
    /// `branch_at_launch` of the latest such session.
    ///
    /// The result set is bounded by three caps to keep the Repository tab from
    /// growing without limit as new worktree-on spawns are recorded:
    ///
    /// - `worktree_base`: absolute path of `$DELTA_WORKTREE_BASE`. A clone
    ///   path is classified as **generated** when it lies under
    ///   `worktree_base` (matched as a `<base>/<child>` prefix — i.e. the path
    ///   begins with `worktree_base + "/"`), and **user** otherwise.
    /// - `active_repo_limit`: only the top-N most-recently-active
    ///   `repo_root`s pass; the rest drop wholesale.
    /// - `user_clone_limit`: per `repo_root`, cap on user-picked path rows.
    /// - `generated_clone_limit`: per `repo_root`, cap on machine-generated
    ///   (under-`worktree_base`) path rows. Kept separate from the user cap so
    ///   a burst of disposable worktrees cannot squeeze out user-meaningful
    ///   clones (the main tree, manual sibling clones).
    ///
    /// Ordering: most-recent first by `last_opened_at`, then by `repo_root`
    /// ASC and `clone_path` ASC for determinism. The interactor de-dups by
    /// `clone_path` again after grouping (the same path can appear under
    /// different `repo_root`s for the same upstream).
    async fn repository_clone_rows(
        &self,
        worktree_base: &str,
        active_repo_limit: i64,
        user_clone_limit: i64,
        generated_clone_limit: i64,
    ) -> Result<Vec<RepositoryCloneRow>>;

    /// The registered clone roots, most-recently-added first.
    ///
    /// Each row is a directory whose direct children the Repository tab will
    /// probe for git clones on every list call. The set is small (one entry per
    /// directory the user has registered) so the whole list is returned at once.
    async fn list_clone_roots(&self) -> Result<Vec<CloneRoot>>;

    /// Register a new clone root. Returns the created row, or
    /// [`crate::Error::CloneRootDuplicate`] when `path` is already registered —
    /// the PRIMARY KEY constraint is the conflict gate, so callers do not need a
    /// pre-check.
    async fn insert_clone_root(&self, path: &str) -> Result<CloneRoot>;

    /// Unregister a clone root. Deleting an unknown path is a no-op (idempotent),
    /// so the Settings dialog's explicit Remove click never surfaces a 404 noise
    /// on a path the user just removed via another tab.
    async fn delete_clone_root(&self, path: &str) -> Result<()>;

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
    ///
    /// Rows carrying the boot-restore marker (`restored_at`) are skipped:
    /// a restored send must never dispatch automatically — not at resume
    /// settle, not at turn end, not on the enqueue idle-flush — until the
    /// user explicitly releases it via [`Self::release_restored_send`].
    async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>>;

    /// A session's open (non-terminal) sends — status `queued` or
    /// `dispatched` — oldest first (ascending `id`).
    ///
    /// This is the server-side truth behind the browser's send strip:
    /// every send accepted for the session that has neither matched a
    /// transcript line nor been cancelled yet. Restored rows (see
    /// [`Self::restore_all_dispatched`]) are included — they are `queued` —
    /// and carry their `restored_at` marker so the UI can render them with
    /// the explicit Send/Cancel affordances instead of a waiting label.
    async fn open_sends(&self, session_id: &SessionId) -> Result<Vec<Send>>;

    /// Promote a `queued` send to `dispatched`, marking it typed so the
    /// normal `UserPromptSubmit` correlation can match it.
    async fn promote_queued_send(&self, id: i64) -> Result<()>;

    /// Return a `dispatched` send to `queued`. A no-op for any other status.
    ///
    /// Used when the turn state machine orphans an outstanding send whose echo
    /// never arrived (see `OrphanedSend::Requeue`): the row keeps its
    /// thread/branch/quote semantics and re-dispatches on the next trigger
    /// that finds the session open and idle — a turn end, an interrupt
    /// ingest, a resume settle, or the enqueue idle-flush (see the use case's
    /// `dispatch_queued_send`) — so a composed message is never silently
    /// lost.
    async fn requeue_send(&self, id: i64) -> Result<()>;

    /// Restore **every** `dispatched` send — across all sessions — to
    /// `queued` with the `restored_at` marker set, returning how many rows
    /// transitioned. Rows in any other status are untouched.
    ///
    /// The boot-time half of the single-outstanding invariant: turn state is
    /// runtime-only and rebuilt `Idle` when the server starts, but `send` rows
    /// are persistent — so a row that was `dispatched` when the previous
    /// process died has no turn machine awaiting its echo and would shadow
    /// [`Self::head_dispatched_send`] correlation forever. The composition
    /// root calls this exactly once at startup, before any session actor
    /// exists (which is what makes the blanket sweep exact: at that moment
    /// every `dispatched` row is an orphan by definition). Restored rather
    /// than cancelled so a composed message is never silently lost — but,
    /// unlike [`Self::requeue_send`]'s plain requeue, a restored row is never
    /// re-dispatched automatically: the message may be days old and the
    /// conversation has moved on, so auto-resending it on the next reopen
    /// would silently re-submit stale text (possibly *after* a newer message
    /// the user just sent). The `restored_at` marker keeps the row out of
    /// [`Self::next_queued_send`] until the user explicitly releases it
    /// ([`Self::release_restored_send`]) or cancels it.
    async fn restore_all_dispatched(&self) -> Result<usize>;

    /// Clear the boot-restore marker of a send, returning it to the normal
    /// queued flow — but **only while it is still `queued` and restored** —
    /// returning whether a row actually transitioned.
    ///
    /// The guarded release half of [`Self::restore_all_dispatched`]: the
    /// `WHERE status = 'queued' AND restored_at IS NOT NULL` clause makes the
    /// transition a no-op (returning `false`) for an unknown id, a row that
    /// was never restored, an already-released row, or one that has since
    /// been cancelled — a clean conflict rather than a clobber. After a
    /// successful release the row is an ordinary `queued` send again and
    /// dispatches through the usual idle triggers.
    async fn release_restored_send(&self, id: i64) -> Result<bool>;

    /// The outstanding dispatched send for a session, if any.
    ///
    /// Under the single-outstanding dispatch rule at most one `dispatched` row
    /// exists per session, so this *is* the send `UserPromptSubmit` correlation
    /// and transcript attribution compare against (by trimmed-text equality at
    /// the call sites). Defined as the oldest `dispatched` row so that, should
    /// the invariant ever be violated, the comparison still deterministically
    /// picks the earliest.
    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>>;

    /// All `dispatched` sends for a session, oldest first (ascending `id`).
    ///
    /// The single-outstanding rule normally caps this list at one element, so
    /// callers that want the head should still prefer
    /// [`Self::head_dispatched_send`]. This is the recovery accessor used when
    /// a re-dispatch routine needs to re-type the *entire* FIFO of currently
    /// `Dispatched` sends — e.g. after Claude Code's auto- or manual `/compact`
    /// swallowed each typed prompt without echoing, leaving any dispatched
    /// row stuck behind a missing echo.
    async fn dispatched_sends(&self, session_id: &SessionId) -> Result<Vec<Send>>;

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

    /// Deny every still-`pending` permission request of a session, recording
    /// `reason` on each row alongside the `denied` status and `decided_at`.
    ///
    /// The disposition for requests whose answer can no longer be delivered —
    /// the agent process ended while approvals were outstanding. `denied` is the
    /// honest record: the tool was never allowed to run, and the provider that
    /// asked is gone, so nothing can act on the row afterwards. Leaving them
    /// `pending` is the failure this exists to prevent: the audit trail would
    /// claim someone is still being asked, and the row would never settle.
    /// `reason` is what keeps the trail readable — the status alone cannot
    /// distinguish a user's Deny from a request nobody could answer.
    ///
    /// Returns the ids of the rows that transitioned (empty when nothing was
    /// pending), so the caller can settle their client-visible notices.
    async fn deny_pending_permission_requests(
        &self,
        session_id: &SessionId,
        reason: &str,
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
    async fn clear_subagent_launch(&self, session_id: &SessionId, tool_use_id: &str) -> Result<()>;

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
    /// picker. `provider` is the provider the option applies to (the caller
    /// defaults it to [`AgentProvider::Claude`] for a back-compat create that
    /// omits it).
    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
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

    /// All registered prompt templates, oldest first (ascending `created_at`,
    /// then ascending id). Stable insertion order: the picker's list must not
    /// reshuffle under the user when a template is edited.
    async fn list_prompt_templates(&self) -> Result<Vec<PromptTemplate>>;

    /// Register a prompt template and return the created row. Both fields are
    /// required and stored verbatim — the caller has already rejected a blank
    /// one — with `updated_at` stamped equal to `created_at`.
    async fn create_prompt_template(&self, label: &str, text: &str) -> Result<PromptTemplate>;

    /// Replace a prompt template's content, re-stamping `updated_at`, and return
    /// the updated row — or `None` if no template has that id. The id and
    /// `created_at` are preserved (a delete+recreate would churn both).
    async fn update_prompt_template(
        &self,
        id: i64,
        label: &str,
        text: &str,
    ) -> Result<Option<PromptTemplate>>;

    /// Delete a prompt template by id. Deleting an unknown id is a no-op.
    async fn delete_prompt_template(&self, id: i64) -> Result<()>;
}

#[async_trait]
impl SessionStore for Box<dyn SessionStore> {
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
        (**self).register_session(new).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_spawning_session(
        &self,
        id: &SessionId,
        cwd: &str,
        branch_at_launch: Option<&str>,
        repo_root: Option<&str>,
        requested_workdir: Option<&str>,
        repository_display_name: Option<&str>,
        provider: AgentProvider,
    ) -> Result<(Session, ThreadId)> {
        (**self)
            .insert_spawning_session(
                id,
                cwd,
                branch_at_launch,
                repo_root,
                requested_workdir,
                repository_display_name,
                provider,
            )
            .await
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

    async fn cwd_exists(&self, path: &str) -> Result<bool> {
        (**self).cwd_exists(path).await
    }

    async fn repository_clone_rows(
        &self,
        worktree_base: &str,
        active_repo_limit: i64,
        user_clone_limit: i64,
        generated_clone_limit: i64,
    ) -> Result<Vec<RepositoryCloneRow>> {
        (**self)
            .repository_clone_rows(
                worktree_base,
                active_repo_limit,
                user_clone_limit,
                generated_clone_limit,
            )
            .await
    }

    async fn list_clone_roots(&self) -> Result<Vec<CloneRoot>> {
        (**self).list_clone_roots().await
    }

    async fn insert_clone_root(&self, path: &str) -> Result<CloneRoot> {
        (**self).insert_clone_root(path).await
    }

    async fn delete_clone_root(&self, path: &str) -> Result<()> {
        (**self).delete_clone_root(path).await
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

    async fn restore_all_dispatched(&self) -> Result<usize> {
        (**self).restore_all_dispatched().await
    }

    async fn release_restored_send(&self, id: i64) -> Result<bool> {
        (**self).release_restored_send(id).await
    }

    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        (**self).head_dispatched_send(session_id).await
    }

    async fn dispatched_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        (**self).dispatched_sends(session_id).await
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
        (**self)
            .decide_permission_request(request_id, allowed)
            .await
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

    async fn deny_pending_permission_requests(
        &self,
        session_id: &SessionId,
        reason: &str,
    ) -> Result<Vec<i64>> {
        (**self)
            .deny_pending_permission_requests(session_id, reason)
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

    async fn clear_subagent_launch(&self, session_id: &SessionId, tool_use_id: &str) -> Result<()> {
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
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        (**self)
            .create_launch_option(label, name, value, default_enabled, provider)
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

    async fn list_prompt_templates(&self) -> Result<Vec<PromptTemplate>> {
        (**self).list_prompt_templates().await
    }

    async fn create_prompt_template(&self, label: &str, text: &str) -> Result<PromptTemplate> {
        (**self).create_prompt_template(label, text).await
    }

    async fn update_prompt_template(
        &self,
        id: i64,
        label: &str,
        text: &str,
    ) -> Result<Option<PromptTemplate>> {
        (**self).update_prompt_template(id, label, text).await
    }

    async fn delete_prompt_template(&self, id: i64) -> Result<()> {
        (**self).delete_prompt_template(id).await
    }
}
