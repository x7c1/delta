use std::io;
use std::path::PathBuf;

/// Why the staged bundle was not put in place of the running one
/// ([`SwapPaths::swap`](crate::swap_paths::SwapPaths::swap)).
#[derive(Debug, thiserror::Error)]
pub enum SwapError {
    /// What an earlier update left at the backup's path could not be
    /// removed; nothing was moved.
    #[error("could not remove {}", backup.display())]
    BackupNotRemoved {
        backup: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The running bundle could not be moved aside; nothing was moved.
    #[error("could not move {} aside to {}", bundle.display(), backup.display())]
    NotMovedAside {
        bundle: PathBuf,
        backup: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The staged bundle could not be moved into place, and the running
    /// one was moved back: the current app is left as it was.
    #[error(
        "could not move the new app from {} to {}, so the current app is left as it was",
        staging.display(),
        bundle.display()
    )]
    NotMovedIn {
        staging: PathBuf,
        bundle: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The staged bundle could not be moved into place (`moved`), nor the
    /// running one moved back from the backup (`source`): no app is left at
    /// the bundle's path.
    #[error(
        "could not move the new app from {} to {} ({moved}), nor move the current app back from {}",
        staging.display(),
        bundle.display(),
        backup.display()
    )]
    NotRestored {
        staging: PathBuf,
        bundle: PathBuf,
        backup: PathBuf,
        moved: io::Error,
        #[source]
        source: io::Error,
    },
}
