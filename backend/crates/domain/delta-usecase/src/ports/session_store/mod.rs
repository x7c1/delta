//! Persisting and querying Delta's thread overlay.
//!
//! [`SessionStore`] is the whole port; the stores it gathers each own one
//! concern of the overlay (session rows, threads, the send FIFO, the message
//! cache, permission requests, and the settings registries), one module each.

use async_trait::async_trait;

use delta_model::{SessionId, ThreadId};

use crate::error::Result;

mod clone_root;
pub use clone_root::CloneRoot;
mod clone_root_store;
pub use clone_root_store::CloneRootStore;
mod launch_option_store;
pub use launch_option_store::LaunchOptionStore;
mod message_store;
pub use message_store::MessageStore;
mod permission_store;
pub use permission_store::PermissionStore;
mod prompt_template_store;
pub use prompt_template_store::PromptTemplateStore;
mod recent_workdir;
pub use recent_workdir::RecentWorkdir;
mod remembered_pane_store;
pub use remembered_pane_store::RememberedPaneStore;
mod repository_clone_row;
pub use repository_clone_row::RepositoryCloneRow;
mod send_store;
pub use send_store::SendStore;
mod session_history_store;
pub use session_history_store::SessionHistoryStore;
mod session_page_row;
pub use session_page_row::SessionPageRow;
mod session_record_store;
pub use session_record_store::SessionRecordStore;
mod subagent_launch_store;
pub use subagent_launch_store::SubagentLaunchStore;
mod thread_store;
pub use thread_store::ThreadStore;

/// Persists and queries Delta's thread overlay.
///
/// This is the irreplaceable data: thread assignment, the semantic-parent
/// graph, the send queue and permission history. Message content and linear
/// parents are a cache rebuildable from the JSONL.
///
/// The methods live on the per-concern stores this trait gathers; anything
/// implementing all of them is a `SessionStore`. A use case bound by
/// `SessionStore` (or holding a `dyn SessionStore`) calls every method of
/// those stores without naming them.
#[async_trait]
pub trait SessionStore:
    SessionRecordStore
    + RememberedPaneStore
    + SessionHistoryStore
    + CloneRootStore
    + ThreadStore
    + SendStore
    + MessageStore
    + PermissionStore
    + SubagentLaunchStore
    + LaunchOptionStore
    + PromptTemplateStore
{
    /// The thread the session's in-progress turn belongs to.
    ///
    /// Hooks that fire *inside* a turn but before its user line is ingested
    /// (`AskUserQuestion`'s `PreToolUse`, the `MessageDisplay` streaming
    /// preview) need to attribute their output to the turn's thread. They
    /// cannot read it from [`ThreadStore::latest_user_thread`] alone: a mid-turn
    /// branch send creates a brand-new thread and is dispatched without its
    /// user line yet in the JSONL (the well-known `UserPromptSubmit` timing
    /// race — the line is attributed later, at `Stop`), so `latest_user_thread`
    /// still reports the *prior* turn's thread.
    ///
    /// The authoritative source before the line lands is the in-flight send
    /// itself: the dispatched head carries the `thread_id` it was composed for.
    /// So resolution prefers, in order:
    ///
    /// 1. the [`SendStore::head_dispatched_send`]'s `thread_id` (the in-flight
    ///    turn's own thread, correct even before its user line is ingested),
    /// 2. [`ThreadStore::latest_user_thread`] (a turn typed straight into the pane,
    ///    with no Delta send driving it, once its line is persisted),
    /// 3. [`ThreadStore::main_thread_id`] (no send and no user line yet — a turn at
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
}

#[async_trait]
impl<T> SessionStore for T where
    T: SessionRecordStore
        + RememberedPaneStore
        + SessionHistoryStore
        + CloneRootStore
        + ThreadStore
        + SendStore
        + MessageStore
        + PermissionStore
        + SubagentLaunchStore
        + LaunchOptionStore
        + PromptTemplateStore
{
}
