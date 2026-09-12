//! Crate-local error type for the composition root.

use thiserror::Error;

/// Errors raised while wiring the application together.
#[derive(Debug, Error)]
pub enum Error {
    /// A host command Delta cannot run without is absent from `PATH`.
    ///
    /// Delta ships as a single binary and drives the host's own tools; `tmux`
    /// is the one it cannot do without at all, since every session of every
    /// provider is launched into a tmux pane. Its absence is caught at startup
    /// rather than deep inside the driver on the first launch. The command name
    /// is carried as data so the server binary can match on the variant and
    /// print one plain line.
    #[error("required command '{bin}' was not found on PATH")]
    MissingCommand {
        /// The command that could not be resolved, exactly as it would be
        /// spawned.
        bin: String,
    },

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
