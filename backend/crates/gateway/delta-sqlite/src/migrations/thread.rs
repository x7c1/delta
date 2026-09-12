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
//!
//! **`last_activity_at` is denormalized on purpose**, exactly as its
//! [`session`](super::session) twin is: it is a copy of the thread's most
//! recent message timestamp (`MAX(message.created_at)`), maintained on every
//! message upsert, and NULL while the thread has no timestamped message. It is
//! derived state, kept because the alternative — the navigator ranking a
//! session's sub-threads by recency — would mean a correlated `MAX` subquery
//! per thread on every threads listing.

use super::Step;

/// The `thread` table's history: the v3 baseline table and the v8 recency
/// column.
pub(super) const STEPS: &[Step] = &[
    Step::additive(
        3,
        "\
CREATE TABLE IF NOT EXISTS thread (
  id               INTEGER PRIMARY KEY,
  session_id       TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  title            TEXT NOT NULL,
  parent_thread_id INTEGER REFERENCES thread(id),
  created_at       TEXT NOT NULL
) STRICT;",
    ),
    // v8: the denormalized per-thread recency, added *and backfilled* in one
    // step. The messages are already in the table, so every existing thread's
    // value is derivable — and leaving them NULL would render as "no sub-thread
    // is newest", which is exactly the reading reserved for "the main thread is
    // where the last message landed". The correlated MAX is backed by
    // `ix_message_thread`.
    Step::additive(
        8,
        "\
ALTER TABLE thread ADD COLUMN last_activity_at TEXT;
UPDATE thread SET last_activity_at =
  (SELECT MAX(m.created_at) FROM message m WHERE m.thread_id = thread.id);",
    ),
];
