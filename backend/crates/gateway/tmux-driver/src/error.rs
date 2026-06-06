//! Crate-local error type for the tmux driver.

use thiserror::Error;

/// Errors raised by [`crate::Tmux`].
#[derive(Debug, Error)]
pub enum Error {
    /// The `tmux` process could not be spawned or waited on.
    #[error("failed to run tmux: {0}")]
    Spawn(#[from] std::io::Error),

    /// `tmux` ran but exited non-zero.
    #[error("tmux exited with status {status}: {stderr}")]
    Command { status: String, stderr: String },
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Tmux(value.to_string())
    }
}
