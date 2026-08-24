//! Crate-local error type for the composition root.

use thiserror::Error;

/// Errors raised while wiring the application together.
#[derive(Debug, Error)]
pub enum Error {
    /// The session store could not be opened.
    #[error("failed to open store: {0}")]
    Store(#[from] delta_sqlite::Error),

    /// The boot-time send reconcile failed: the sweep that returns every
    /// `dispatched` row orphaned by the previous process to `queued` could
    /// not run. Fatal at startup — booting without it would leave zombie
    /// rows shadowing `UserPromptSubmit` correlation.
    #[error("failed to requeue dispatched sends at boot: {0}")]
    BootReconcile(#[from] delta_usecase::Error),

    /// The boot-time launch-option reconcile failed: the sweep that
    /// materializes Delta's declared launch-option presets into the registry
    /// could not run. Fatal at startup rather than logged and skipped — it is
    /// a plain write against a store that has just been opened and migrated, so
    /// a failure here means something is wrong with the database, not with the
    /// catalog. Carrying on would open Settings with an arbitrary subset of the
    /// shipped options present.
    #[error("failed to reconcile built-in launch options at boot: {0}")]
    BuiltinLaunchOptions(delta_usecase::Error),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
