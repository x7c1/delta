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

use super::Step;

/// The `launch_option` table's history: the v3 baseline table.
pub(super) const STEPS: &[Step] = &[Step::additive(
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
)];
