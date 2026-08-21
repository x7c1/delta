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
//! **`restored_at`** marks a `queued` row recovered at boot from a `dispatched`
//! state a dead server process left behind. A restored row is never dispatched
//! automatically (the queued-selection queries filter `restored_at IS NULL`); it
//! stays visible in the open-send list until the user explicitly releases it
//! (clearing the marker) or cancels it. NULL on the normal send path, and NULL
//! on any row written before the column existed — which is exactly the "not
//! restored" meaning, so pre-upgrade queued rows keep dispatching normally.
//!
//! The queue is part of the irreplaceable overlay: a send that has not been
//! matched yet exists nowhere else.

use super::Step;

/// The `send` table's history: the v3 baseline table, then its status index.
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
];
