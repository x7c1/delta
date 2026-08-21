//! The `permission_request` table: the history of tool-permission prompts and
//! how they were decided.
//!
//! `tool_use_id` is nullable: NULL means the request has no correlating tool
//! call id (never an empty-string sentinel). `ix_permission_request_tool_use`
//! backs the lookup by `(session_id, tool_use_id)` that resolves a request when
//! the correlated `tool_result` is ingested.
//!
//! The decision history is part of the irreplaceable overlay — it is Delta's
//! own record, not something the transcript can be re-read to recover.

use super::Step;

/// The `permission_request` table's history: the v3 baseline table, then the
/// index its correlation lookup walks.
pub(super) const STEPS: &[Step] = &[
    Step::additive(
        3,
        "\
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
) STRICT;",
    ),
    Step::additive(
        3,
        "\
CREATE INDEX IF NOT EXISTS ix_permission_request_tool_use
  ON permission_request(session_id, tool_use_id);",
    ),
];
