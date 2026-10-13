//! Why an update was not installed.

use std::path::PathBuf;

/// Why an update was not installed.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// The user dismissed the request for an administrator's password.
    /// Nothing went wrong: the update stays ready to install.
    #[error("the request for an administrator's password was dismissed")]
    Dismissed,
    /// Delta cannot install the update itself on this machine: on Linux, the
    /// program that asks for the password or the one that installs is
    /// missing, no authentication agent is running, or the user is not
    /// authorized; on macOS, the app does not run from a `Delta.app` it may
    /// replace, or `hdiutil` is missing. The user can still install it by
    /// hand.
    #[error("Delta cannot install the update itself: {0}")]
    Unavailable(String),
    /// The program the install needs is there but could not be started.
    /// Delta cannot install the update itself, as for
    /// [`Self::Unavailable`]; the user can still install it by hand.
    #[error("Delta cannot install the update itself: could not run {}", program.display())]
    Unrunnable {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The installer rejected the file itself: not a regular file, not the
    /// one the release's digest names, or not Delta at the version it was
    /// asked for. The file is not to be installed by any means, by hand
    /// included; the text is the installer's own one-line reason.
    #[error("{0}")]
    Rejected(String),
    /// The installer ran and could not install the file for a reason other
    /// than the file itself (the release out of reach to check it against,
    /// the package manager or a disk image tool failing, the new app bundle
    /// not renamed into place); the text is the installer's own one-line
    /// reason. The user can still install it by hand.
    #[error("{0}")]
    Failed(String),
}
