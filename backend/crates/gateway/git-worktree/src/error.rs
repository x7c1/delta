//! Crate-local error type for the git worktree gateway.

use thiserror::Error;

/// Errors raised by [`crate::Git`].
#[derive(Debug, Error)]
pub enum Error {
    /// The `git` process could not be spawned or waited on.
    #[error("failed to run git: {0}")]
    Spawn(#[from] std::io::Error),

    /// `git` ran but exited non-zero on a command whose failure is a real
    /// error (not an expected "absent" signal). Carries the failing command
    /// and git's stderr so the cause is visible.
    #[error("git {command} exited with status {status}: {stderr}")]
    Command {
        command: String,
        status: String,
        stderr: String,
    },
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Git(value.to_string())
    }
}
