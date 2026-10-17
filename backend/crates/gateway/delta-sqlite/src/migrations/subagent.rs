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
//! **What the running indicator needs** (v12). A launch row also carries what a
//! re-adopted session needs to light the running-subagent indicator again when
//! its pane survived a restart of Delta: the launch line is then behind the
//! stored cursor and is never folded again, so the in-memory running set can
//! only be rebuilt from here. `drives_indicator` says whether the fold lit the
//! indicator for this launch — a background `Agent`/`Task` call or a forked
//! skill did, a background `Bash` (recorded only so its completion is
//! attributed) did not — and `subagent_type` / `description` are what the
//! indicator shows. There is no `background` column: only background launches
//! leave a row (a foreground subagent cannot outlive its turn), so every row
//! is one.
//!
//! The step backfills the rows a previous binary left outstanding, so a
//! subagent launched before an upgrade still lights the indicator after it: a
//! forked skill is recognised by its synthetic `forked-skill:` id (its display
//! fields are not in any column, so they read NULL), and an `Agent`/`Task`
//! launch by its `tool_use` block in the cached `message.content_json`, which
//! also yields its `subagent_type` and `description` when they are strings.
//! Any other row — a background `Bash`, or a launch whose line never reached
//! the message cache — keeps `drives_indicator = 0` and is never lit.
//!
//! Rows cascade on session delete.

use super::Step;

/// The `subagent_launch` table's history: the v3 baseline table and the v12
/// running-indicator columns.
pub(super) const STEPS: &[Step] = &[
    Step::additive(
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
    ),
    // v12: what the running indicator needs, added and backfilled in one step
    // (see the module docs). The flag is a constant-default `NOT NULL`, which
    // `ADD COLUMN` accepts without rewriting the table; the two display fields
    // are nullable, and NULL is also what a launch that carried none reads.
    // The backfill reads only the messages of sessions that still have an
    // outstanding launch, and `json_each` is fed `[]` for a malformed or NULL
    // `content_json` so one bad cache row cannot fail the migration.
    Step::additive(
        12,
        "\
ALTER TABLE subagent_launch ADD COLUMN drives_indicator INTEGER NOT NULL DEFAULT 0
  CHECK (drives_indicator IN (0, 1));
ALTER TABLE subagent_launch ADD COLUMN subagent_type TEXT;
ALTER TABLE subagent_launch ADD COLUMN description TEXT;
UPDATE subagent_launch SET drives_indicator = 1 WHERE tool_use_id LIKE 'forked-skill:%';
UPDATE subagent_launch
   SET drives_indicator = 1,
       subagent_type = launch.subagent_type,
       description = launch.description
  FROM (
    SELECT m.session_id AS session_id,
           json_extract(block.value, '$.id') AS tool_use_id,
           CASE json_type(block.value, '$.input.subagent_type')
             WHEN 'text' THEN json_extract(block.value, '$.input.subagent_type')
           END AS subagent_type,
           CASE json_type(block.value, '$.input.description')
             WHEN 'text' THEN json_extract(block.value, '$.input.description')
           END AS description
      FROM message m,
           json_each(CASE WHEN json_valid(m.content_json) THEN m.content_json ELSE '[]' END)
             AS block
     WHERE m.session_id IN (SELECT session_id FROM subagent_launch)
       AND block.type = 'object'
       AND json_extract(block.value, '$.type') = 'tool_use'
       AND json_extract(block.value, '$.name') IN ('Agent', 'Task')
  ) AS launch
 WHERE launch.session_id = subagent_launch.session_id
   AND launch.tool_use_id = subagent_launch.tool_use_id;",
    ),
];
