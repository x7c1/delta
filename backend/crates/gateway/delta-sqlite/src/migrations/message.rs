//! The `message` table: the cached transcript body, its indexes, and the
//! full-text index kept in sync with it.
//!
//! **Composite primary key.** Transcript uuids are only trusted to be unique per
//! transcript — compaction or forking could repeat a uuid across sessions — so
//! the key is `(session_id, uuid)`. `created_at` is nullable: a transcript line
//! without a timestamp stores NULL, not a sentinel value.
//!
//! **The role vocabulary** is pinned to `delta_model::Role::as_str` (and its
//! wire twin `WireRole`). `compact_summary` is the synthetic line Claude Code
//! writes when `/compact` runs; the attribution fold produces and persists it,
//! so it must be an accepted value here. Widening this constraint is a change
//! SQLite cannot apply to an existing table in place (a `CHECK` edit needs a
//! full table rebuild), so it ships as a *destructive* step on the ladder — a
//! table rebuild, not an `IF NOT EXISTS` edit that an existing database would
//! silently ignore.
//!
//! **Metadata columns.** `model`, `git_branch`, `cwd` and `response_time_ms` are
//! transcript-derived per-message metadata and all nullable: older lines and
//! non-assistant shapes carry none. `response_time_ms` is REAL because the turn
//! duration is a JSON number. `provider_item_id` is the provider's own id for the
//! source item (Codex's `item.id`), carried so a streaming delta and its final
//! message id-join in place; NULL for Claude and for any message with no
//! provider item. All of these are cache rebuildable from the JSONL transcript,
//! like the rest of the message body — a row that predates them reads NULL and
//! re-ingest fills them.
//!
//! **Indexes.** `ix_message_session_created` backs the per-session
//! `MAX(created_at)` used to (re)compute a session's denormalized
//! `last_activity_at` on message upsert; it is a single-session lookup, so the
//! index bounds it. The others back the per-thread and per-parent reads the
//! attribution fold and the thread view issue.
//!
//! **Full-text index** (groundwork: no search UI yet). `message_fts` is an
//! external-content fts5 table over `message.content_text`, keyed by the message
//! table's rowid (a STRICT table still has a rowid unless WITHOUT ROWID). fts5
//! virtual tables cannot themselves be STRICT; that is fine. External-content
//! fts5 requires explicit `'delete'` entries carrying the OLD text before a row
//! is removed or rewritten, which is what the delete and update triggers do; the
//! update trigger fires on any column change because an upsert rewrites the
//! whole row (`content_text` included).

use super::Step;

/// The `message` table's history: the v3 baseline table, its indexes, and the
/// FTS index with its synchronising triggers. The index and trigger steps follow
/// the table step, which is the order the registry preserves within a version.
pub(super) const STEPS: &[Step] = &[
    Step::additive(
        3,
        "\
CREATE TABLE IF NOT EXISTS message (
  session_id           TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  uuid                 TEXT NOT NULL,
  thread_id            INTEGER NOT NULL REFERENCES thread(id),
  role                 TEXT NOT NULL
                         CHECK (role IN
                           ('user','assistant','system','meta','compact_summary','other')),
  linear_parent_uuid   TEXT,
  semantic_parent_uuid TEXT,
  prompt_id            TEXT,
  seq                  INTEGER NOT NULL,
  content_text         TEXT,
  content_json         TEXT,
  created_at           TEXT,
  model                TEXT,
  git_branch           TEXT,
  cwd                  TEXT,
  response_time_ms     REAL,
  provider_item_id     TEXT,
  PRIMARY KEY (session_id, uuid)
) STRICT;",
    ),
    Step::additive(
        3,
        "\
CREATE INDEX IF NOT EXISTS ix_message_session_seq ON message(session_id, seq);
CREATE INDEX IF NOT EXISTS ix_message_session_created ON message(session_id, created_at);
CREATE INDEX IF NOT EXISTS ix_message_thread ON message(thread_id);
CREATE INDEX IF NOT EXISTS ix_message_semantic_parent ON message(semantic_parent_uuid);",
    ),
    Step::additive(
        3,
        "\
CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
  content_text, content='message', content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS message_fts_after_insert AFTER INSERT ON message BEGIN
  INSERT INTO message_fts(rowid, content_text) VALUES (new.rowid, new.content_text);
END;
CREATE TRIGGER IF NOT EXISTS message_fts_after_delete AFTER DELETE ON message BEGIN
  INSERT INTO message_fts(message_fts, rowid, content_text)
    VALUES ('delete', old.rowid, old.content_text);
END;
CREATE TRIGGER IF NOT EXISTS message_fts_after_update AFTER UPDATE ON message BEGIN
  INSERT INTO message_fts(message_fts, rowid, content_text)
    VALUES ('delete', old.rowid, old.content_text);
  INSERT INTO message_fts(rowid, content_text) VALUES (new.rowid, new.content_text);
END;",
    ),
];
