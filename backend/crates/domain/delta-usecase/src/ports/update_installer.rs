//! Installing a downloaded, verified update of the desktop app.
//!
//! The [`UpdateInstaller`] port replaces the installed app with the file a
//! download verified. Whether there is such a file, and which release it is,
//! is the release update's job ([`crate::ReleaseUpdate`]); the installer only
//! installs what it is handed, through whatever the platform needs to do so
//! (on Linux, a root helper started through polkit).

use std::path::{Path, PathBuf};

use async_trait::async_trait;

/// Why an update was not installed.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// The user dismissed the request for an administrator's password.
    /// Nothing went wrong: the update stays ready to install.
    #[error("the request for an administrator's password was dismissed")]
    Dismissed,
    /// Delta cannot install the update itself on this machine: the program
    /// that asks for the password or the one that installs is missing, no
    /// authentication agent is running, or the user is not authorized. The
    /// user can still install it from a terminal.
    #[error("Delta cannot install the update itself: {0}")]
    Unavailable(String),
    /// The program that asks for the password is there but could not be
    /// started. Delta cannot install the update itself, as for
    /// [`Self::Unavailable`]; the user can still install it from a terminal.
    #[error("Delta cannot install the update itself: could not run {}: {source}", program.display())]
    Unrunnable {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The installer rejected the file itself: not a regular file, not the
    /// one the release's digest names, or not Delta at the version it was
    /// asked for. The file is not to be installed by any means, from a
    /// terminal included; the text is the installer's own one-line reason.
    #[error("{0}")]
    Rejected(String),
    /// The installer ran and could not install the file for a reason other
    /// than the file itself (the release out of reach to check it against,
    /// the package manager failing); the text is the installer's own one-line
    /// reason. The user can still install it from a terminal.
    #[error("{0}")]
    Failed(String),
}

/// Installs downloaded updates.
#[async_trait]
pub trait UpdateInstaller: Send + Sync {
    /// Install `file`, the verified download of release `version`
    /// (`v<version>`), over the installed app. Resolves once the install has
    /// ended, which includes the time the user takes to authenticate. `Ok`
    /// also when the installed app is that version or newer already, so
    /// there was nothing to install.
    async fn install(&self, version: &str, file: &Path) -> Result<(), InstallError>;
}
