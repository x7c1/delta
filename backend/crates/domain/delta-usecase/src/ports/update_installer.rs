//! Installing a downloaded, verified update of the desktop app.
//!
//! The [`UpdateInstaller`] port replaces the installed app with the file a
//! download verified. Whether there is such a file, and which release it is,
//! is the release update's job ([`crate::ReleaseUpdate`]); the installer only
//! installs what it is handed, through whatever the platform needs to do so
//! (on Linux, a root helper started through polkit; on macOS, replacing the
//! running `Delta.app` with the one in the downloaded disk image, as the
//! user).

use std::path::Path;

use async_trait::async_trait;

use super::{InstallError, InstalledApp};

/// Installs downloaded updates.
#[async_trait]
pub trait UpdateInstaller: Send + Sync {
    /// Install `file`, the download of release `version` (`v<version>`)
    /// verified against the lowercase hex `sha256`, over the installed app,
    /// and name the app to start to run it. Resolves once the install has
    /// ended, which includes the time the user takes to authenticate. `Ok`
    /// also when the installed app is that version or newer already, so
    /// there was nothing to install.
    async fn install(
        &self,
        version: &str,
        file: &Path,
        sha256: &str,
    ) -> Result<InstalledApp, InstallError>;

    /// Why this installer cannot install any update on this machine, as far
    /// as it can tell without installing one and without side effects, or
    /// `None` when it may (the default: an installer that can only tell by
    /// trying). Asked once, when the update is set up at startup, so the
    /// browser shows the way by hand instead of an Install that can only end
    /// [`InstallError::Unavailable`]; [`Self::install`] still checks again,
    /// in case the machine changed since.
    fn unavailable(&self) -> Option<String> {
        None
    }
}
