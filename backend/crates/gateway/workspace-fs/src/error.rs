//! Crate-local error type for the filesystem workspace gateway.

use thiserror::Error;

/// Errors raised by [`crate::FsWorkspace`].
#[derive(Debug, Error)]
pub enum Error {
    /// The working directory or settings file could not be created or written.
    #[error("workspace io error: {0}")]
    Io(#[from] std::io::Error),

    /// A user-selected path does not resolve to an existing directory (it is
    /// missing, not a directory, or could not be canonicalized).
    #[error("invalid working directory: {0}")]
    InvalidWorkdir(String),

    /// A directory could not be read because the process lacks permission.
    #[error("permission denied: {0}")]
    Permission(String),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for delta_usecase::Error {
    fn from(value: Error) -> Self {
        match value {
            // I/O during settings writes is still an internal workspace failure.
            Error::Io(_) => delta_usecase::Error::Workspace(value.to_string()),
            Error::InvalidWorkdir(msg) => delta_usecase::Error::InvalidWorkdir(msg),
            Error::Permission(msg) => delta_usecase::Error::WorkdirPermission(msg),
        }
    }
}
