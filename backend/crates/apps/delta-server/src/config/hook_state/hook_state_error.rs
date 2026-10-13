//! A failure to read or write the hook state file.

use std::path::PathBuf;

/// A failure to read or write the hook state file.
///
/// Fatal at startup: the server cannot know which secret to accept without the
/// file, and a fresh secret it cannot record would silently change on every
/// restart. Each variant names the path, since the remedy — fixing the
/// directory's permissions, or deleting the file — is the user's.
#[derive(Debug, thiserror::Error)]
pub enum HookStateError {
    /// The file exists but could not be read.
    #[error(
        "could not read the hook state file {}: {source}. Fix its permissions, \
         or delete it to have Delta create a new one",
        path.display()
    )]
    Read {
        /// The state file.
        path: PathBuf,
        /// What reading it failed with.
        source: std::io::Error,
    },
    /// The file was found readable by others and could not be restricted to
    /// its owner.
    #[error(
        "could not restrict the hook state file {} to its owner: {source}. \
         Delete it to have Delta create a new one",
        path.display()
    )]
    Restrict {
        /// The state file.
        path: PathBuf,
        /// What changing its permissions failed with.
        source: std::io::Error,
    },
    /// The file could not be written.
    #[error("could not write the hook state file {}: {source}", path.display())]
    Write {
        /// The state file.
        path: PathBuf,
        /// What writing it failed with.
        source: std::io::Error,
    },
}
