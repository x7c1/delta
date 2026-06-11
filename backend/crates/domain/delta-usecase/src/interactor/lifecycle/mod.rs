//! Session lifecycle use cases: spawn, ensure, resume (open), and close.

mod close_session;
mod dispatch_ready_resumes;
mod ensure_session;
mod mint_free_token;
mod new_session;
mod open_session;
mod reap_stale_spawns;
mod spawn_fresh;
mod workdir_for;

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
