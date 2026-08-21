//! The `subagent_launch` table: outstanding background-task launches.
//!
//! The launching thread of each `run_in_background` Agent/Task/Bash, keyed by
//! the launching tool_use id. Such a call returns immediately and its completion
//! is injected later — frequently in a different sync window — as a
//! `<task-notification>` user line carrying the same id. Persisting
//! `(session_id, tool_use_id) -> thread_id` lets the attribution fold reseed and
//! attribute that notification back to the thread that launched the task instead
//! of whatever thread is current when it lands. A row is inserted when the launch
//! is first seen and deleted when its notification is folded, so the table holds
//! only still-outstanding launches.
//!
//! `task_id` is the background-task identifier Claude Code mints for the
//! subagent, learned from the launching tool's `tool_result` via the
//! `PostToolUse(Agent)` hook (the row is inserted earlier with `task_id` NULL).
//! Recent Claude Code versions sometimes drop `<tool-use-id>` from the user
//! message `<task-notification>` body while keeping `<task-id>`, so this is the
//! fallback correlation key that lets the fold still finish the running subagent
//! in that case. A launch recorded before the column existed reads NULL and keeps
//! the legacy tool-use-id-only behaviour.
//!
//! Rows cascade on session delete.

use super::Step;

/// The `subagent_launch` table's history: the v3 baseline table.
pub(super) const STEPS: &[Step] = &[Step::additive(
    3,
    "\
CREATE TABLE IF NOT EXISTS subagent_launch (
  session_id  TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  tool_use_id TEXT NOT NULL,
  thread_id   INTEGER NOT NULL REFERENCES thread(id),
  task_id     TEXT,
  created_at  TEXT NOT NULL,
  PRIMARY KEY (session_id, tool_use_id)
) STRICT;",
)];
