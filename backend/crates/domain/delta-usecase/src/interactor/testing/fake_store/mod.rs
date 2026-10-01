//! In-memory [`SessionStore`] fake backing the interactor use-case tests.
//!
//! The fake's state and shared stamps live here; the store's behaviour is
//! split by area into sibling modules (sessions, session history, threads,
//! sends, messages, permissions, subagent launches, launch options, prompt
//! templates, clone roots), with [`session_store`] wiring them into the trait.
//!
//! [`SessionStore`]: crate::ports::SessionStore

mod clone_roots;
mod launch_options;
mod messages;
mod permissions;
mod prompt_templates;
mod sends;
mod session_history;
mod session_store;
mod sessions;
mod subagents;
mod threads;

use std::collections::HashMap;
use std::sync::Mutex;

use delta_attribution::SubagentLaunch;
use delta_model::{
    LaunchOption, Message, PermissionRequest, PromptTemplate, Send, Session, SessionId, Thread,
};

use crate::ports::CloneRoot;

/// The creation timestamp every fake-created row carries. Fixed so assertions
/// can name it; the real store stamps the wall clock.
const FAKE_CREATED_AT: &str = "2026-01-01T00:00:00Z";

/// The timestamp a fake update re-stamps `updated_at` with. Distinct from
/// [`FAKE_CREATED_AT`] so a test can tell an edited row from an untouched one —
/// which the real store's second-resolution clock cannot guarantee within a
/// single test.
const FAKE_UPDATED_AT: &str = "2026-01-02T00:00:00Z";

/// The `held_at` stamp both hold producers write (the boot restore and the
/// echo-deadline park). Fixed for the same reason as [`FAKE_CREATED_AT`]: a
/// test asserts the marker's presence, never its value.
const HELD_AT: &str = "2026-01-01T00:00:00Z";

#[derive(Default)]
pub(crate) struct FakeStoreInner {
    pub(crate) sessions: Vec<Session>,
    pub(crate) threads: Vec<Thread>,
    pub(crate) next_thread_id: i64,
    pub(crate) sends: Vec<Send>,
    pub(crate) next_send_id: i64,
    pub(crate) messages: Vec<Message>,
    pub(crate) permissions: Vec<PermissionRequest>,
    pub(crate) next_perm_id: i64,
    pub(crate) transcript_lines_read: HashMap<SessionId, usize>,
    pub(crate) launch_options: Vec<LaunchOption>,
    pub(crate) next_launch_option_id: i64,
    pub(crate) prompt_templates: Vec<PromptTemplate>,
    pub(crate) next_prompt_template_id: i64,
    /// Outstanding background-task launches keyed by `(session_id,
    /// tool_use_id)`, mirroring the SQL `subagent_launch` table. The value is
    /// the `SubagentLaunch` carrying the launching thread plus the optional
    /// `task_id` learned via the `PostToolUse(Agent)` hook.
    pub(crate) subagent_launches: HashMap<(SessionId, String), SubagentLaunch>,
    pub(crate) clone_roots: Vec<CloneRoot>,
    /// When set, [`SessionStore::cancel_send`](crate::ports::SessionStore::cancel_send) fails with a store error, so a
    /// test can make the `TurnInput::Close` of a session with an unechoed send
    /// fail at its row write.
    pub(crate) fail_cancel_send: bool,
    /// When set, [`SessionStore::clear_subagent_launch`](crate::ports::SessionStore::clear_subagent_launch) fails with a store
    /// error for this tool_use id (and succeeds for every other), so a test
    /// can make the process-gone subagent sweep hit a failing row write.
    pub(crate) fail_clear_subagent_launch_for: Option<String>,
}

#[derive(Default)]
pub(crate) struct FakeStore {
    pub(crate) inner: Mutex<FakeStoreInner>,
}
