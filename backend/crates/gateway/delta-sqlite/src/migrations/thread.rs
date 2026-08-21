//! The `thread` table: the branch structure Delta layers over a transcript.
//!
//! A thread's root message (the message it branches from) is deliberately NOT
//! stored here: the canonical home of the branch edge is
//! `message.semantic_parent_uuid`, and the root is derived from the thread's
//! first semantically parented message (or from its recorded send, before that
//! message is ingested).
//!
//! `parent_thread_id` is a self-reference, so a thread branched off another
//! thread carries the edge directly. Threads cascade on session delete.

use super::Step;

/// The `thread` table's history: the v3 baseline table.
pub(super) const STEPS: &[Step] = &[Step::additive(
    3,
    "\
CREATE TABLE IF NOT EXISTS thread (
  id               INTEGER PRIMARY KEY,
  session_id       TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  title            TEXT NOT NULL,
  parent_thread_id INTEGER REFERENCES thread(id),
  created_at       TEXT NOT NULL
) STRICT;",
)];
