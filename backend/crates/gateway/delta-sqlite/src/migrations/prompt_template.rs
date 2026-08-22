//! The `prompt_template` table: the prompt-template registry.
//!
//! A named block of instruction text the user registers once and later inserts
//! into the composer at the cursor, instead of retyping or pasting the same long
//! instructions ("once CI is green, merge and then update the plan doc…"). Each
//! row is one `(label, text)` record: `label` names it in the picker, `text` is
//! what gets inserted verbatim.
//!
//! The text is deliberately **provider-independent** — it is prose the user
//! types into a composer, not argv or a request field — so unlike
//! [`launch_option`](super::launch_option) there is no `provider` column and the
//! registry is global.
//!
//! This table is session-independent (no foreign key, never cascaded): the
//! registry outlives any individual session, and it is irreplaceable — nothing
//! else records what the user wrote.
//!
//! **Column notes.**
//!
//! - `text` is stored verbatim, including leading and trailing whitespace and
//!   newlines: a template may deliberately end with a newline, and the insertion
//!   point in the composer is where that matters. Only the emptiness check
//!   trims.
//! - `updated_at` exists because this registry's `PATCH` edits content rather
//!   than flipping a flag, so "when was this last reworded" is a question the
//!   row can answer. It is stamped equal to `created_at` on insert.

use super::Step;

/// The `prompt_template` table's history: created at v4.
pub(super) const STEPS: &[Step] = &[Step::additive(
    4,
    "\
CREATE TABLE IF NOT EXISTS prompt_template (
  id         INTEGER PRIMARY KEY,
  label      TEXT NOT NULL,
  text       TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;",
)];
