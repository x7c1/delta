//! Crate-local error type for the SQLite gateway.

use thiserror::Error;

/// Errors raised by [`crate::SqliteStore`].
#[derive(Debug, Error)]
pub enum Error {
    /// A `rusqlite` failure.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A stored value did not parse back into a domain type.
    #[error("invalid stored value: {0}")]
    Decode(#[from] delta_model::Error),

    /// An expected row was missing.
    #[error("not found: {0}")]
    NotFound(String),

    /// The database was written by a *newer* binary: its `PRAGMA user_version`
    /// is above this binary's [`crate::SCHEMA_VERSION`]. Raised by the startup
    /// gate so the server exits cleanly with a `make reset` hint, instead of
    /// letting the mismatch surface later as confusing runtime errors. The
    /// migration ladder only runs forward, so there is nothing to apply here —
    /// the remedy is a newer binary, or rebuilding the overlay.
    #[error(
        "delta SQLite overlay schema version mismatch: \
         database is at version {found}, this binary expects version {expected}. \
         The overlay was written by a newer delta and cannot be migrated \
         backwards. Run that newer delta, or `make reset` to rebuild the overlay."
    )]
    SchemaMismatch {
        /// The version stored in the on-disk file's `PRAGMA user_version`.
        found: u32,
        /// The version this binary was built against ([`crate::SCHEMA_VERSION`]).
        expected: u32,
    },

    /// The database has delta's tables but no `PRAGMA user_version` stamp, so it
    /// predates the schema gate entirely (a v0.1.0 overlay). Its real shape is
    /// unknown — the ladder's baseline cannot be safely replayed onto it, and
    /// there is no version to migrate forward from — so the open is refused.
    #[error(
        "delta SQLite overlay predates the schema version gate: \
         the database has delta's tables but no schema version stamp, \
         so it cannot be migrated forward. \
         Run `make reset` to rebuild the overlay."
    )]
    UnstampedOverlay,

    /// The database is stamped *below* the ladder's oldest step. Those older
    /// generations were squashed into the baseline rather than reconstructed, so
    /// no step describes the distance from such a file to the baseline. Applying
    /// the baseline anyway would be silent corruption — its statements are all
    /// `CREATE ... IF NOT EXISTS`, so they would no-op over the tables that are
    /// already there, leaving the file's real (older) shape untouched while
    /// stamping it current. Refused instead, the way the pre-ladder gate refused
    /// any version it did not recognise.
    #[error(
        "delta SQLite overlay is older than the migration ladder: \
         database is at version {found}, and the ladder's oldest step produces \
         version {baseline}, so no step upgrades it. \
         Run `make reset` to rebuild the overlay."
    )]
    PreBaselineOverlay {
        /// The version stored in the on-disk file's `PRAGMA user_version`.
        found: u32,
        /// The lowest version the compiled-in ladder can start from.
        baseline: u32,
    },

    /// The compiled-in migration ladder is internally inconsistent — a gap
    /// between versions, or a highest step that disagrees with
    /// [`crate::SCHEMA_VERSION`]. A programming error, caught on open (and, well
    /// before that, by the registry test) rather than left to silently skip a
    /// step that would never be applied to anything.
    #[error("delta SQLite migration ladder is inconsistent: {0}")]
    InvalidLadder(String),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Store(value.to_string())
    }
}
