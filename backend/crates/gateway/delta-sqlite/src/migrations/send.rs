//! The `send` table: the outgoing-send queue, and the index its status lookups
//! walk.
//!
//! **Status vocabulary.**
//!
//! - `queued` — recorded, not yet typed into the pane (held while a turn is in
//!   flight).
//! - `dispatched` — typed into the pane, awaiting the matching
//!   `UserPromptSubmit`.
//! - `matched` — correlated to its transcript message uuid.
//! - `cancelled` — abandoned (rolled back, superseded, or timed out).
//!
//! **`held_at`** marks a `queued` row as *held in the queue until the user
//! releases it*. Two paths stamp it, both recovering a row that was
//! `dispatched` with no one left to await its echo: the boot-time restore, for
//! a row a dead server process left behind, and the echo-deadline park, for a
//! row whose keystrokes were swallowed without a trace twice running. A held
//! row is never dispatched automatically (the queued-selection queries filter
//! `held_at IS NULL`); it stays visible in the open-send list until the user
//! explicitly releases it (clearing the marker) or cancels it. NULL on the
//! normal send path, and NULL on any row written before the column existed —
//! which is exactly the "not held" meaning, so pre-upgrade queued rows keep
//! dispatching normally.
//!
//! The column shipped in the v3 baseline as `restored_at`, back when the boot
//! restore was its only producer. **v6 renames it to `held_at`**: the park
//! stamps the same marker, so a name describing one producer had become a name
//! that lies about the column's meaning. The rename is the whole of that step —
//! no row changes hands, and every read path moves with it.
//!
//! The queue is part of the irreplaceable overlay: a send that has not been
//! matched yet exists nowhere else.

use super::Step;

/// The `send` table's history: the v3 baseline table, its status index, then
/// the v6 rename of the hold marker.
pub(super) const STEPS: &[Step] = &[
    Step::additive(
        3,
        "\
CREATE TABLE IF NOT EXISTS send (
  id                   INTEGER PRIMARY KEY,
  session_id           TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  thread_id            INTEGER NOT NULL REFERENCES thread(id),
  semantic_parent_uuid TEXT,
  text                 TEXT NOT NULL,
  locator_quote        TEXT,
  status               TEXT NOT NULL
                         CHECK (status IN ('queued','dispatched','matched','cancelled')),
  matched_uuid         TEXT,
  created_at           TEXT NOT NULL,
  restored_at          TEXT
) STRICT;",
    ),
    Step::additive(
        3,
        "CREATE INDEX IF NOT EXISTS ix_send_session_status ON send(session_id, status);",
    ),
    // Nothing is copied or backfilled: SQLite renames the column in place and
    // every existing value, marker or NULL, keeps its meaning under the new
    // name.
    Step::destructive(6, "ALTER TABLE send RENAME COLUMN restored_at TO held_at;"),
];

/// The v6 rename undone — the one place the retired column name may still be
/// written.
///
/// `crate::store::tests::schema` builds a previous-generation database by
/// undoing every step above the version under test; that is what lets it prove
/// the real ladder carries such a file forward. Keeping the SQL next to the
/// step means the historical name stays inside the migration history, which is
/// the only place it belongs now.
#[cfg(test)]
pub(crate) const UNDO_HELD_AT_RENAME: &str =
    "ALTER TABLE send RENAME COLUMN held_at TO restored_at;";
