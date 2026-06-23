//! The database schema, created in its final form on open.
//!
//! The schema below is what a *fresh* database looks like: a single batch of
//! `CREATE ... IF NOT EXISTS` statements. Development databases are recreated
//! with `make reset`, so most schema changes need nothing more than editing the
//! `CREATE` statements here and resetting.
//!
//! Additive column changes that must survive an *existing* database (so a user
//! is not forced to reset and lose their irreplaceable thread overlay) are
//! handled by the idempotent, guarded `ALTER TABLE` steps in
//! [`crate::store::SqliteStore::init`] (see [`ADDITIVE_COLUMNS`]). A fresh
//! database already has those columns from the `CREATE` statements, so the
//! guarded step is a no-op there; an old database gains the column and is
//! backfilled in the same open.
//!
//! [`SCHEMA_VERSION`] is the binary's expected on-disk schema generation,
//! reflected into the SQLite file via `PRAGMA user_version`. The startup gate
//! in [`crate::store::SqliteStore::init`] compares the two and refuses to
//! continue on mismatch — see the compatibility policy doc for the rationale.
//!
//! Every table is `STRICT` (values must match the declared column types) and
//! value domains are pinned with `CHECK` constraints, so a typo'd status or a
//! mistyped bind surfaces as an immediate error instead of silently persisted
//! garbage. Child tables cascade on session delete, so removing a session row
//! removes everything it owns.

/// The on-disk schema generation this binary expects.
///
/// Reflected into the SQLite file via `PRAGMA user_version` and checked on
/// every open. Bump this whenever a destructive change ships (column dropped,
/// table renamed, constraint tightened, etc.) so a stale overlay fails loud and
/// early on startup with a `make reset` hint, instead of surfacing as confusing
/// runtime errors mid-session. Additive changes that go through
/// [`ADDITIVE_COLUMNS`] do *not* require a bump — those are transparently
/// applied on open to an existing DB.
///
/// See the compatibility policy doc for the full rule set.
pub const SCHEMA_VERSION: u32 = 1;

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
--
-- `last_activity_at` is a denormalized copy of the session's most recent
-- message timestamp (`MAX(message.created_at)`), maintained on every message
-- upsert. It is NULL while the session has no timestamped message. The session
-- list orders by it directly so the ordering is index-backed
-- (`ix_session_last_activity`) and a LIMIT truly bounds the work, instead of
-- recomputing recency for every session with a correlated subquery and sorting
-- the whole table. The navigator's recency key is
-- `COALESCE(last_activity_at, created_at)`, so a message-less session still
-- sorts on its own `created_at`.
CREATE TABLE IF NOT EXISTS session (
  id                TEXT PRIMARY KEY,
  cwd               TEXT NOT NULL,
  transcript_path   TEXT,
  title             TEXT,
  status            TEXT NOT NULL
                      CHECK (status IN ('spawning','active','ended','failed')),
  created_at        TEXT NOT NULL,
  last_activity_at  TEXT,
  -- Spawn-time snapshot of the local git branch checked out in `cwd` and the
  -- repository root that contained it. Both are NULL when the launch directory
  -- was not inside a git repo (or HEAD was detached). Both are additive and
  -- arrived after the table first shipped (see `ADDITIVE_COLUMNS`), so an
  -- existing database gains them as NULL on every pre-existing row with no
  -- backfill — the navigator's frontend falls back to the cwd basename then.
  branch_at_launch  TEXT,
  repo_root         TEXT
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
  -- Transcript-derived per-message metadata (all nullable: older lines and
  -- non-assistant shapes carry none). `response_time_ms` is REAL because the
  -- turn duration is a JSON number. These are cache rebuildable from the JSONL
  -- transcript, like the rest of the message body.
  model                TEXT,
  git_branch           TEXT,
  cwd                  TEXT,
  response_time_ms     REAL,
  PRIMARY KEY (session_id, uuid)
) STRICT;

CREATE INDEX IF NOT EXISTS ix_message_session_seq ON message(session_id, seq);
-- Backs the per-session `MAX(created_at)` used to (re)compute a session's
-- denormalized `last_activity_at` on message upsert and to backfill it for an
-- existing database. Both are single-session lookups, so the index bounds them.
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

-- The launch-option registry: custom `claude` CLI flags the user can register
-- once and later multi-select when starting a session. Each row is one flat
-- `(label?, name, value?)` record — a generic flag pass-through where `name` is
-- the flag (e.g. '--plugin-dir', '--permission-mode') and `value` its argument
-- (e.g. '/path/to/plugins', 'auto'). `value` is nullable for valueless flags;
-- a repeatable flag is stored as multiple separate rows. `label` is an optional
-- human-friendly note for the row. This table is session-independent (no
-- foreign key, never cascaded): the registry outlives any individual session.
-- `default_enabled` (0/1) marks an option to start pre-checked in the
-- session-start picker. It is additive (see `ADDITIVE_COLUMNS`): an existing
-- database gains it via a guarded `ALTER TABLE ... ADD COLUMN ... DEFAULT 0`,
-- so every pre-existing row defaults to off with no backfill.
CREATE TABLE IF NOT EXISTS launch_option (
  id              INTEGER PRIMARY KEY,
  label           TEXT,
  name            TEXT NOT NULL,
  value           TEXT,
  default_enabled INTEGER NOT NULL DEFAULT 0 CHECK (default_enabled IN (0, 1)),
  created_at      TEXT NOT NULL
) STRICT;

-- Outstanding background-task launches: the launching thread of each
-- `run_in_background` Agent/Task/Bash, keyed by the launching tool_use id. Such
-- a call returns immediately and its completion is injected later — frequently
-- in a different sync window — as a `<task-notification>` user line carrying the
-- same id. Persisting `(session_id, tool_use_id) -> thread_id` lets the
-- attribution fold reseed and attribute that notification back to the thread
-- that launched the task instead of whatever thread is current when it lands. A
-- row is inserted when the launch is first seen and deleted when its
-- notification is folded, so the table holds only still-outstanding launches.
--
-- `task_id` is the background-task identifier Claude Code mints for the
-- subagent, learned from the launching tool's `tool_result` via the
-- `PostToolUse(Agent)` hook (the row is inserted earlier with task_id NULL).
-- Recent Claude Code versions sometimes drop `<tool-use-id>` from the user
-- message `<task-notification>` body while keeping `<task-id>`, so this is the
-- fallback correlation key that lets the fold still finish the running
-- subagent in that case. It is additive (see `ADDITIVE_COLUMNS`), so an existing
-- database gains it as NULL on every pre-existing row with no backfill — a
-- launch that predates the upgrade keeps the legacy tool-use-id-only behaviour.
CREATE TABLE IF NOT EXISTS subagent_launch (
  session_id  TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  tool_use_id TEXT NOT NULL,
  thread_id   INTEGER NOT NULL REFERENCES thread(id),
  task_id     TEXT,
  created_at  TEXT NOT NULL,
  PRIMARY KEY (session_id, tool_use_id)
) STRICT;

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

/// Index backing the session-list page ordering.
///
/// Created after `last_activity_at` exists (see
/// [`crate::store::SqliteStore::apply_additive_columns`]) rather than in
/// [`SCHEMA_SQL`], because it references that column. It is an **expression**
/// index on `COALESCE(last_activity_at, created_at)` — the navigator's recency
/// key — so the page query's `ORDER BY COALESCE(last_activity_at, created_at)
/// DESC, created_at DESC, id DESC` is satisfied by walking the index in order
/// and stopping after LIMIT rows, instead of recomputing recency for every
/// session and sorting the whole table in a temp b-tree. A plain
/// `(last_activity_at, created_at, id)` index would NOT match, because the sort
/// key is the COALESCE expression, not the bare column.
pub const RECENCY_INDEX_SQL: &str = "\
    CREATE INDEX IF NOT EXISTS ix_session_recency \
      ON session(COALESCE(last_activity_at, created_at) DESC, created_at DESC, id DESC)";

/// Columns added to an existing table after the table first shipped, applied on
/// open as guarded `ALTER TABLE ... ADD COLUMN` steps.
///
/// SQLite has no `ADD COLUMN IF NOT EXISTS`, so each step is gated on the column
/// being absent (checked via `PRAGMA table_info`). A fresh database already has
/// the column from [`SCHEMA_SQL`], making the step a no-op; an existing database
/// gains it. `ADD COLUMN` can add either a nullable column without a default
/// (`last_activity_at`, backfilled by [`BACKFILL_LAST_ACTIVITY_SQL`] for rows
/// that predate it) or a `NOT NULL` column with a *constant* default
/// (`launch_option.default_enabled DEFAULT 0`, which needs no backfill — every
/// pre-existing row simply takes the constant 0 = off).
pub const ADDITIVE_COLUMNS: &[AdditiveColumn] = &[
    AdditiveColumn {
        table: "session",
        column: "last_activity_at",
        add_column_sql: "ALTER TABLE session ADD COLUMN last_activity_at TEXT",
    },
    AdditiveColumn {
        table: "launch_option",
        column: "default_enabled",
        add_column_sql:
            "ALTER TABLE launch_option ADD COLUMN default_enabled INTEGER NOT NULL DEFAULT 0",
    },
    // Per-message transcript metadata, added to `message` after it first
    // shipped. All nullable with no default, so an existing database gains them
    // as NULL on every pre-existing row (and re-ingest of newer lines fills them
    // — they are transcript-derived cache, not irreplaceable overlay).
    AdditiveColumn {
        table: "message",
        column: "model",
        add_column_sql: "ALTER TABLE message ADD COLUMN model TEXT",
    },
    AdditiveColumn {
        table: "message",
        column: "git_branch",
        add_column_sql: "ALTER TABLE message ADD COLUMN git_branch TEXT",
    },
    AdditiveColumn {
        table: "message",
        column: "cwd",
        add_column_sql: "ALTER TABLE message ADD COLUMN cwd TEXT",
    },
    AdditiveColumn {
        table: "message",
        column: "response_time_ms",
        add_column_sql: "ALTER TABLE message ADD COLUMN response_time_ms REAL",
    },
    // Spawn-time git snapshot, added to `session` after it first shipped. Both
    // are nullable with no default: an existing database gains them as NULL on
    // every pre-existing row, so a session launched before this change stays
    // unidentified by branch/repo and the navigator falls back to the cwd
    // basename. No backfill — we cannot recover what `git rev-parse` would have
    // reported at the historical spawn moment.
    AdditiveColumn {
        table: "session",
        column: "branch_at_launch",
        add_column_sql: "ALTER TABLE session ADD COLUMN branch_at_launch TEXT",
    },
    AdditiveColumn {
        table: "session",
        column: "repo_root",
        add_column_sql: "ALTER TABLE session ADD COLUMN repo_root TEXT",
    },
    // Background-task identifier learned via `PostToolUse(Agent)`, added to
    // `subagent_launch` after it first shipped. Nullable with no default: an
    // existing database gains it as NULL on every pre-existing row, so a launch
    // that predates the upgrade stays correlated only by tool_use_id (the
    // legacy behaviour, still correct when the notification carries that id).
    AdditiveColumn {
        table: "subagent_launch",
        column: "task_id",
        add_column_sql: "ALTER TABLE subagent_launch ADD COLUMN task_id TEXT",
    },
];

/// One additive column and the `ALTER TABLE` that introduces it.
pub struct AdditiveColumn {
    /// The table the column belongs to.
    pub table: &'static str,
    /// The column name, used to detect whether it already exists.
    pub column: &'static str,
    /// The `ALTER TABLE ... ADD COLUMN` applied when the column is absent.
    pub add_column_sql: &'static str,
}

/// Backfill `session.last_activity_at` from each session's most recent message
/// timestamp, falling back to NULL when the session has no timestamped message
/// (the navigator then orders that session on its own `created_at`). Idempotent:
/// it overwrites with the same computed value, so running it on an
/// already-current database changes nothing. Run once after the column is added
/// for an existing database; a fresh database has no rows yet, so it is inert.
pub const BACKFILL_LAST_ACTIVITY_SQL: &str = "\
    UPDATE session SET last_activity_at = \
      (SELECT MAX(m.created_at) FROM message m WHERE m.session_id = session.id)";
