//! Session lifecycle use cases: spawn, resume (open), close, and the
//! launch-readiness ticks. The `ensure`/`new` entry points live on the
//! interactor's routing layer (they mint the session id and pick the actor);
//! everything here runs inside a session's actor.

mod adapter_launch;
mod adapter_session;
mod close_session;
mod dispatch_ready_resumes;
mod finish_launch;
mod launch_prep;
mod mint_free_token;
mod open_session;
mod reap_stale_spawns;
mod record_launched_pane;
mod spawn_fresh;
mod workdir_for;
mod worktree_launch_dir;

pub(in crate::interactor) use adapter_launch::PreparedAdapterLaunch;
pub(in crate::interactor) use record_launched_pane::LaunchApproval;
pub(in crate::interactor) use spawn_fresh::FreshSpawn;

#[cfg(test)]
mod tests;

/// The `--resume` flag passed to `claude` to reattach to a stored conversation.
const RESUME_FLAG: &str = "--resume";

/// The `--session-id` flag passed to `claude` to pin a fresh conversation's
/// `session_id` to a value Delta mints up front. With the id known at spawn
/// time, the first `UserPromptSubmit` hook reports exactly that id, so a fresh
/// spawn correlates to its session by id — never by working directory.
const SESSION_ID_FLAG: &str = "--session-id";

/// The `--settings` flag passed to `claude` to load Delta's session settings
/// (hooks + theme) from a Delta-owned file, instead of writing them into the
/// session's working directory and risking a clobber of a real project's
/// `.claude/settings.json`.
const SETTINGS_FLAG: &str = "--settings";
