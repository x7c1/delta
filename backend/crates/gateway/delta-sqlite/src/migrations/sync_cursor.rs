//! The `sync_cursor` table: the transcript-ingestion cursor.
//!
//! How many lines of the JSONL transcript have been consumed, split out of
//! `session` because it is ingestion runtime state, not part of the session
//! entity — keeping it in its own table stops it from churning the session row
//! and keeps the domain `Session` free of it.
//!
//! The row cascades on session delete, like every other child table.

use super::Step;

/// The `sync_cursor` table's history: the v3 baseline table.
pub(super) const STEPS: &[Step] = &[Step::additive(
    3,
    "\
CREATE TABLE IF NOT EXISTS sync_cursor (
  session_id TEXT PRIMARY KEY REFERENCES session(id) ON DELETE CASCADE,
  lines_read INTEGER NOT NULL DEFAULT 0 CHECK (lines_read >= 0)
) STRICT;",
)];
