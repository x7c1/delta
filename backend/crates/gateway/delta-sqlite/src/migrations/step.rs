//! One rung of the migration ladder.

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

/// A single schema change and the `PRAGMA user_version` it produces.
///
/// Steps live in the per-subject modules next to the rest of that subject's
/// history, and the registry ([`crate::migrations::registry`]) orders them
/// globally by [`Step::to_version`]. Several steps may share one version — the
/// v3 baseline is nothing but such a group — and every step of a version is
/// applied inside that version's single transaction.
#[derive(Debug, Clone, Copy)]
pub struct Step {
    /// The `PRAGMA user_version` the database is at once this step (and every
    /// other step of the same version) has been applied.
    pub to_version: u32,
    /// Whether applying this step can destroy data that is already there.
    pub kind: StepKind,
    /// The SQL applied, run as a batch so one step may hold several statements
    /// that belong together (a table and its triggers, say).
    pub sql: &'static str,
}

impl Step {
    /// A step that only adds: `ADD COLUMN`, `CREATE TABLE`, `CREATE INDEX`,
    /// `CREATE TRIGGER`. Pending sets made only of these take no backup.
    pub const fn additive(to_version: u32, sql: &'static str) -> Self {
        Self {
            to_version,
            kind: StepKind::Additive,
            sql,
        }
    }

    /// A step that rewrites, renames, drops, tightens or moves existing data.
    /// A pending set containing one of these triggers a pre-migration snapshot.
    ///
    /// Nothing on the shipped ladder is destructive yet — it was built before
    /// the first change that needs it, so that the change itself only has to
    /// append a step. The runner's tests drive it, hence the `cfg_attr`: the
    /// dead-code warning stays live for the test build, and comes back for the
    /// library the moment the first real destructive step lands and is removed.
    #[cfg_attr(not(test), allow(dead_code))]
    pub const fn destructive(to_version: u32, sql: &'static str) -> Self {
        Self {
            to_version,
            kind: StepKind::Destructive,
            sql,
        }
    }
}
