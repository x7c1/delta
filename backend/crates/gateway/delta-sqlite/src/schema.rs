//! The database schema, applied as a migration on open.

/// All `CREATE TABLE`/`CREATE INDEX` statements, idempotent via `IF NOT EXISTS`.
///
/// Timestamps are ISO-8601 text (SQLite has no native datetime). The thread
/// overlay — `thread_id`, `semantic_parent_uuid`, threads, the send queue and
/// permission history — is the irreplaceable data; message content and the
/// linear parent are a cache rebuildable from the JSONL transcript.
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS session (
  id TEXT PRIMARY KEY,
  cwd TEXT NOT NULL,
  transcript_path TEXT NOT NULL,
  title TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  transcript_lines_read INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS thread (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES session(id),
  title TEXT NOT NULL,
  parent_thread_id INTEGER REFERENCES thread(id),
  root_message_uuid TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS message (
  uuid TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES session(id),
  thread_id INTEGER NOT NULL REFERENCES thread(id),
  role TEXT NOT NULL,
  linear_parent_uuid TEXT,
  semantic_parent_uuid TEXT,
  prompt_id TEXT,
  seq INTEGER NOT NULL,
  content_text TEXT,
  content_json TEXT,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_message_session_seq ON message(session_id, seq);
CREATE INDEX IF NOT EXISTS ix_message_thread ON message(thread_id);
CREATE INDEX IF NOT EXISTS ix_message_semantic_parent ON message(semantic_parent_uuid);

CREATE TABLE IF NOT EXISTS pending_send (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES session(id),
  thread_id INTEGER NOT NULL REFERENCES thread(id),
  semantic_parent_uuid TEXT,
  text TEXT NOT NULL,
  locator_quote TEXT,
  status TEXT NOT NULL DEFAULT 'pending',
  matched_uuid TEXT,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_pending_send_status ON pending_send(session_id, status);

CREATE TABLE IF NOT EXISTS permission_request (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES session(id),
  tool_name TEXT NOT NULL,
  tool_input_json TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  decision_reason TEXT,
  created_at TEXT NOT NULL,
  decided_at TEXT
);
"#;
