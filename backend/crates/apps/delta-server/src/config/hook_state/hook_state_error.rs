//! A failure to read or write the hook state file, or to adopt the hook secret.

use std::path::PathBuf;

/// A failure to read or write the hook state file, or to adopt the hook secret.
///
/// Fatal at startup: the server cannot know which secret to accept without the
/// file, a fresh secret it cannot record would silently change on every
/// restart, and a secret not of the form Delta mints is not one it adopts. Each
/// variant names its source (the file's path, or `DELTA_HOOK_SECRET`), since
/// the remedy — fixing the directory's permissions, deleting the file, or
/// unsetting the variable — is the user's.
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
    /// `DELTA_HOOK_SECRET` is set to something other than a hook secret of the
    /// form Delta mints.
    #[error(
        "DELTA_HOOK_SECRET is not a usable hook secret: it must be 64 lowercase \
         hexadecimal characters. Unset it to use the secret Delta keeps in its \
         data directory"
    )]
    MalformedExplicitSecret,
    /// The state file records a hook secret that is not of the form Delta
    /// mints.
    #[error(
        "the hook state file {} holds a hook secret that is not 64 lowercase \
         hexadecimal characters. Delete the file to have Delta mint a new one",
        path.display()
    )]
    MalformedRecordedSecret {
        /// The state file.
        path: PathBuf,
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
