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

    /// Creating the parent directory for a worktree path failed. `git worktree
    /// add <path>` requires the parent of `<path>` to already exist; the worktree
    /// base lives outside any repo tree and may not have been created yet.
    /// Carries the directory whose creation failed.
    #[error("failed to create worktree base directory {path}: {source}")]
    WorktreeBaseIo {
        path: String,
        source: std::io::Error,
    },

    /// Reading, writing, or renaming Claude Code's user config file failed.
    /// Carries the config path so the failing file is visible.
    #[error("trust config I/O failed at {path}: {source}")]
    TrustIo {
        path: String,
        source: std::io::Error,
    },

    /// The existing user config file is not valid JSON. It is left untouched
    /// rather than overwritten, so a corrupt or hand-edited file is never
    /// clobbered. Carries the path and the parse error.
    #[error("trust config at {path} is not valid JSON: {source}")]
    TrustParse {
        path: String,
        source: serde_json::Error,
    },

    /// The updated user config could not be serialized back to JSON.
    #[error("failed to serialize trust config for {path}: {source}")]
    TrustSerialize {
        path: String,
        source: serde_json::Error,
    },
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        delta_usecase::Error::Git(value.to_string())
    }
}
