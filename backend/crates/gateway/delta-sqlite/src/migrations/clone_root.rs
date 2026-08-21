//! The `clone_root` table: registered clone roots.
//!
//! Directories where the user's git clones live. The Repository tab probes each
//! one's direct children for git clones, surfacing clones the user has not yet
//! launched a session in (the "umbrella session" case where `session.repo_root`
//! is the umbrella's path and the actual sub-repos never get a row of their own).
//!
//! One row per registered path. The table is session-independent (no foreign
//! key, never cascaded) and is only ever rewritten through the dedicated CRUD
//! endpoints; what the user registered lives nowhere else, so it is part of the
//! irreplaceable overlay.
//!
//! An earlier generation declared this table under a different name. That rename
//! shipped before the ladder existed, as a `SCHEMA_VERSION` bump that sent an
//! existing database to `make reset`; it is one of the changes the v3 baseline
//! squashes. Under the ladder the same change would be a destructive step —
//! `ALTER TABLE ... RENAME TO ...` carrying the rows across — because
//! `CREATE TABLE IF NOT EXISTS` can never rename anything.

use super::Step;

/// The `clone_root` table's history: the v3 baseline table.
pub(super) const STEPS: &[Step] = &[Step::additive(
    3,
    "\
CREATE TABLE IF NOT EXISTS clone_root (
  path        TEXT PRIMARY KEY,
  created_at  TEXT NOT NULL
) STRICT;",
)];
