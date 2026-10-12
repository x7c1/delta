//! What starts the app again once an update is installed over it.

use std::path::PathBuf;

/// The installed app an update was installed as, which the desktop shell
/// starts again to restart into the update.
///
/// The installer names it when it installs, from where it put the app, so
/// the restart never asks the running process where it was started from:
/// once the app is replaced, that may name a deleted file or a backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstalledApp {
    /// An executable to run (Linux: `/usr/bin/delta-desktop`, where the
    /// `.deb` installs it).
    Executable(PathBuf),
    /// A macOS app bundle (`…/Delta.app`) to start with `open`, as Finder
    /// would.
    Bundle(PathBuf),
}
