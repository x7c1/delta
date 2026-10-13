//! What a migration step does to an existing database.

#[cfg(doc)]
use super::Step;

/// What a [`Step`] does to an existing database.
///
/// The distinction drives exactly one behaviour — whether the runner takes a
/// pre-migration snapshot before applying the pending set (see
/// [`crate::migrations::migrate`]) — and nothing else. It is deliberately a
/// choice made at the call site (via [`Step::additive`] / [`Step::destructive`])
/// rather than a flag an author has to remember to set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// Adds something without touching existing rows: `ADD COLUMN`, or creating
    /// an index, trigger or table. An interrupted or reverted additive step
    /// cannot lose data that was already there, so no snapshot is taken.
    ///
    /// SQLite narrows what `ADD COLUMN` accepts: either a nullable column with
    /// no default, or a `NOT NULL` column whose default is a *constant*. A
    /// non-constant default, or `NOT NULL` without one, is rejected outright —
    /// giving those columns a value on rows that predate them takes a second
    /// statement in the same step (an `UPDATE` backfill), or a rebuild, which is
    /// [`Destructive`](Self::Destructive).
    Additive,
    /// Rewrites or removes what is already there: a table rebuild (the only way
    /// SQLite can edit a `CHECK`, a column type or a constraint), a rename, a
    /// `DROP`, or any statement that moves data between columns or tables. The
    /// transaction protects against a *failed* step, but not against a step that
    /// succeeds and turns out to have been wrong, so these ship with a snapshot.
    Destructive,
}
