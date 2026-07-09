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
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
