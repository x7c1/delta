//! The database schema, created in its final form on open.
//!
//! There is NO migration machinery: the schema below is what a database looks
//! like, period. Development databases are recreated with `make reset`, so a
//! schema change here never needs a guarded `ALTER TABLE` — change the
//! `CREATE` statements and reset.
//!
//! Every table is `STRICT` (values must match the declared column types) and
//! value domains are pinned with `CHECK` constraints, so a typo'd status or a
//! mistyped bind surfaces as an immediate error instead of silently persisted
//! garbage. Child tables cascade on session delete, so removing a session row
//! removes everything it owns.

/// All `CREATE TABLE`/`CREATE INDEX`/`CREATE TRIGGER` statements, idempotent
/// via `IF NOT EXISTS`.
///
/// Timestamps are ISO-8601 UTC text (SQLite has no native datetime). The
/// thread overlay — `thread_id`, `semantic_parent_uuid`, threads, the send
/// queue and permission history — is the irreplaceable data; message content
/// and the linear parent are a cache rebuildable from the JSONL transcript.
pub const SCHEMA_SQL: &str = r#"
-- Status lifecycle: a Delta-launched session is INSERTed as 'spawning' when
-- the id is minted (before `claude` is up), flips to 'active' when the first
-- hook binds the spawn, and becomes 'failed' if the spawn never binds before
-- its deadline (a failed session with zero ingested messages is deleted at
-- reap time instead). `transcript_path` is NULL while 'spawning': the path is
-- owned by Claude Code and only learned from the first hook.
CREATE TABLE IF NOT EXISTS session (
  id              TEXT PRIMARY KEY,
  cwd             TEXT NOT NULL,
  transcript_path TEXT,
  title           TEXT,
  status          TEXT NOT NULL
                    CHECK (status IN ('spawning','active','ended','failed')),
  -- 1 while a turn is in flight (a send was dispatched / a turn started and no
  -- Stop or interrupt has been observed since). Branch/quoted sends issued
  -- while this is set are queued rather than dispatched mid-turn.
  turn_active     INTEGER NOT NULL DEFAULT 0 CHECK (turn_active IN (0,1)),
  created_at      TEXT NOT NULL
) STRICT;

-- The transcript-ingestion cursor, split out of `session`: how many lines of
-- the JSONL transcript have been consumed. This is ingestion runtime state,
-- not part of the session entity — keeping it in its own table stops it from
-- churning the session row and keeps the domain `Session` free of it.
CREATE TABLE IF NOT EXISTS sync_cursor (
  session_id TEXT PRIMARY KEY REFERENCES session(id) ON DELETE CASCADE,
  lines_read INTEGER NOT NULL DEFAULT 0 CHECK (lines_read >= 0)
) STRICT;

-- A thread's root message (the message it branches from) is NOT stored here:
-- the canonical home of the branch edge is `message.semantic_parent_uuid`,
-- and the root is derived from the thread's first semantically parented
-- message (or its recorded send, before that message is ingested).
CREATE TABLE IF NOT EXISTS thread (
  id               INTEGER PRIMARY KEY,
  session_id       TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  title            TEXT NOT NULL,
  parent_thread_id INTEGER REFERENCES thread(id),
  created_at       TEXT NOT NULL
) STRICT;

-- Composite primary key: transcript uuids are only trusted to be unique per
-- transcript — compaction or forking could repeat a uuid across sessions.
-- `created_at` is nullable: a transcript line without a timestamp stores NULL,
-- not a sentinel value.
CREATE TABLE IF NOT EXISTS message (
  session_id           TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  uuid                 TEXT NOT NULL,
  thread_id            INTEGER NOT NULL REFERENCES thread(id),
  role                 TEXT NOT NULL
                         CHECK (role IN ('user','assistant','system','meta','other')),
  linear_parent_uuid   TEXT,
  semantic_parent_uuid TEXT,
  prompt_id            TEXT,
  seq                  INTEGER NOT NULL,
  content_text         TEXT,
  content_json         TEXT,
  created_at           TEXT,
  PRIMARY KEY (session_id, uuid)
) STRICT;

CREATE INDEX IF NOT EXISTS ix_message_session_seq ON message(session_id, seq);
-- Speeds up the per-session MAX(created_at) the session-list page query runs to
-- derive each row's recency (last activity) inline.
CREATE INDEX IF NOT EXISTS ix_message_session_created ON message(session_id, created_at);
CREATE INDEX IF NOT EXISTS ix_message_thread ON message(thread_id);
CREATE INDEX IF NOT EXISTS ix_message_semantic_parent ON message(semantic_parent_uuid);

-- The outgoing-send queue. Status vocabulary:
--   queued     recorded, not yet typed into the pane (held while a turn is
--              in flight)
--   dispatched typed into the pane, awaiting the matching `UserPromptSubmit`
--   matched    correlated to its transcript message uuid
--   cancelled  abandoned (rolled back, superseded, or timed out)
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
  created_at           TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS ix_send_session_status ON send(session_id, status);

-- `tool_use_id` is nullable: NULL means the request has no correlating tool
-- call id (never an empty-string sentinel).
CREATE TABLE IF NOT EXISTS permission_request (
  id              INTEGER PRIMARY KEY,
  session_id      TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  tool_name       TEXT NOT NULL,
  tool_input_json TEXT NOT NULL,
  tool_use_id     TEXT,
  status          TEXT NOT NULL CHECK (status IN ('pending','allowed','denied')),
  decision_reason TEXT,
  created_at      TEXT NOT NULL,
  decided_at      TEXT
) STRICT;

-- Resolving a permission request looks it up by (session_id, tool_use_id) when
-- the correlated tool_result is ingested.
CREATE INDEX IF NOT EXISTS ix_permission_request_tool_use
  ON permission_request(session_id, tool_use_id);

-- Full-text index over message content (groundwork: no search UI yet).
-- External-content fts5 over `message.content_text`, keyed by the message
-- table's rowid (a STRICT table still has a rowid unless WITHOUT ROWID).
-- fts5 virtual tables cannot themselves be STRICT; that is fine.
CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
  content_text, content='message', content_rowid='rowid'
);

-- Keep the FTS index in sync with `message`. External-content fts5 requires
-- explicit 'delete' entries carrying the OLD text before a row is removed or
-- rewritten; the update trigger fires on any column change because an upsert
-- rewrites the whole row (content_text included).
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
END;
"#;
