//! The `launch_option` table: the launch-option registry.
//!
//! Custom CLI flags (and their non-Claude equivalents) the user registers once
//! and later multi-selects when starting a session. Each row is one flat
//! `(label?, name, value?)` record — a generic pass-through where `name` is the
//! flag (e.g. `--plugin-dir`, `--permission-mode`) and `value` its argument
//! (e.g. `/path/to/plugins`, `auto`). `value` is nullable for valueless flags;
//! a repeatable flag is stored as multiple separate rows. `label` is an optional
//! human-friendly note for the row.
//!
//! This table is session-independent (no foreign key, never cascaded): the
//! registry outlives any individual session, and it is irreplaceable — nothing
//! else records what the user registered.
//!
//! **Column notes.**
//!
//! - `default_enabled` (0/1) marks an option to start pre-checked in the
//!   session-start picker. `NOT NULL DEFAULT 0`, so a row written before the
//!   column existed simply reads as off.
//! - `provider` names which provider the option applies to. Claude options are
//!   argv flags (`--plugin-dir`, `--permission-mode`, …); other providers
//!   register their own option set (e.g. Codex `thread/start` fields).
//!   `NOT NULL DEFAULT 'claude'` so every pre-existing row and any insert that
//!   omits it stays a Claude option.
//! - `builtin_key` is `NULL` for a row the user registered and non-null for one
//!   Delta *ships* — a declared launch-option preset materialized into this
//!   table at startup. It is both the marker (the API refuses to delete such a
//!   row; the UI badges it) and the reconciliation key: startup matches a
//!   declared preset to its row by this value and updates the row in place, so
//!   a shipped row's id survives across restarts and stays usable in a saved
//!   selection. Nullable and added by `ALTER TABLE`, so every pre-existing row
//!   reads as the user's own. Unique via `ux_launch_option_builtin_key`, so one
//!   key can never name two rows; SQLite treats NULLs as distinct in a unique
//!   index, which leaves the user's own rows unconstrained.

use super::Step;

/// The `launch_option` table's history: the v3 baseline table, plus the v5
/// `builtin_key` marker for the rows Delta ships.
pub(super) const STEPS: &[Step] = &[
    Step::additive(
        3,
        "\
CREATE TABLE IF NOT EXISTS launch_option (
  id              INTEGER PRIMARY KEY,
  label           TEXT,
  name            TEXT NOT NULL,
  value           TEXT,
  default_enabled INTEGER NOT NULL DEFAULT 0 CHECK (default_enabled IN (0, 1)),
  created_at      TEXT NOT NULL,
  provider        TEXT NOT NULL DEFAULT 'claude'
) STRICT;",
    ),
    // Both statements are one step: SQLite cannot add a `UNIQUE` column via
    // `ALTER TABLE`, so the uniqueness has to arrive as a separate index — and a
    // step's SQL runs as one batch in one transaction, so the column and its
    // index can never end up half-applied.
    Step::additive(
        5,
        "\
ALTER TABLE launch_option ADD COLUMN builtin_key TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS ux_launch_option_builtin_key
  ON launch_option(builtin_key);",
    ),
];
