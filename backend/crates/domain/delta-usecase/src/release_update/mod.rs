//! Downloading the newer release the release check found, installing it,
//! and restarting into it.
//!
//! [`ReleaseUpdate`] takes an in-app update through its steps: it downloads
//! this platform's asset of the newer release into the update directory and
//! verifies its sha256; on Linux and macOS it then installs the verified file
//! through the [`UpdateInstaller`] and lets the app restart into what the
//! installer installed. Elsewhere the download is as far as it goes.
//!
//! # Who may update
//!
//! Only a desktop app built by the release workflow ([`NotOffered::of_build`]):
//! the CLI server cannot replace itself, and a desktop app built locally from a
//! newer tree would be rolled back by the release. The server enforces this on
//! every request, not only by what it tells the browser to offer
//! ([`UpdateOffer`]). Update is offered only for a newer release this app
//! would download ([`downloadable_asset`]), so a release it would refuse never
//! shows an Update that can only fail.
//!
//! # What is downloaded
//!
//! Exactly the release the browser was told about (the [`NewerRelease`] the
//! check recorded, assets included), and of it the asset
//! [`downloadable_asset`] picks: the one named for this platform
//! ([`choose_asset`]), from a URL under [`RELEASE_DOWNLOAD_PREFIX`]
//! ([`check_download_url`]), stating a sha256 digest. The [`AssetDownloader`]
//! verifies the file against that digest.
//!
//! # What is installed
//!
//! Only a verified download of the newer release that is ready: the install
//! is handed that file, that release's version and the sha256 the file was
//! verified against, never anything a request names. The installer checks
//! the file again on its own side. When it
//! rejects the file itself ([`UpdateInstall::Rejected`]), the file is removed
//! and the release has to be downloaded again: a file that failed the check
//! is never offered for install, from a terminal included. When Delta cannot
//! install it ([`UpdateInstall::Unavailable`]) or the install fails for
//! another reason ([`UpdateInstall::Failed`]), the state carries how the user
//! installs the file by hand ([`ManualInstall`]: a command for their own
//! terminal on Linux, the disk image to open on macOS). Where the installer
//! can tell at startup that it cannot install any update here (on macOS, the
//! app does not run from a `Delta.app` this user may replace), Install is not
//! offered at all, and a ready download comes with the way by hand instead
//! ([`InstallUnavailable`]). A dismissed password prompt leaves the download
//! ready, as if nothing had been asked.
//!
//! # One at a time
//!
//! The transfer and the install each run in the background and their states
//! ([`UpdateDownload`], [`UpdateInstall`]) are held here for the browser to
//! read. A request while one runs joins it rather than starting a second; a
//! request after a failure starts over; a request after the same release is
//! ready (or installed) answers that without doing it again.

mod asset_choice;
pub use asset_choice::{
    check_download_url, choose_asset, downloadable_asset, RELEASE_DOWNLOAD_PREFIX,
};
mod build_origin;
pub use build_origin::BuildOrigin;
mod install_unavailable;
pub use install_unavailable::InstallUnavailable;
mod launcher;
pub use launcher::Launcher;
mod manual_install;
pub use manual_install::ManualInstall;
mod not_offered;
pub use not_offered::NotOffered;
mod platform;
pub use platform::Platform;
mod update_download;
pub use update_download::UpdateDownload;
mod update_install;
pub use update_install::UpdateInstall;
mod update_offer;
pub use update_offer::UpdateOffer;
mod update_refusal;
pub use update_refusal::UpdateRefusal;
mod update_restart;
pub use update_restart::UpdateRestart;

mod install;
mod start;

#[cfg(test)]
mod testing;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::ports::{AssetDownloader, InstallError, UpdateInstaller};
use crate::NewerRelease;

/// The in-app update: whether it is offered, and the state of the last
/// download and install.
pub struct ReleaseUpdate {
    availability: Availability,
    download: Arc<Mutex<Option<UpdateDownload>>>,
    install: Arc<Mutex<Option<UpdateInstall>>>,
}

enum Availability {
    NotOffered(NotOffered),
    Offered(Offered),
}

/// What an app that may update updates with.
struct Offered {
    platform: Platform,
    dir: PathBuf,
    downloader: Arc<dyn AssetDownloader>,
    installer: Arc<dyn UpdateInstaller>,
    /// Why the installer cannot install any update here, as it said when
    /// the update was set up ([`UpdateInstaller::unavailable`]); `None`
    /// where it may, and where the platform installs nothing in the app.
    install_unavailable: Option<String>,
}

impl ReleaseUpdate {
    /// Updates offered: `platform`'s asset is downloaded into `dir` (the data
    /// directory's `updates/`) through `downloader`, and installed through
    /// `installer` where the app installs updates itself.
    ///
    /// Where it does, asks `installer` now, once, whether it cannot install
    /// any update here ([`UpdateInstaller::unavailable`]); if so, the app
    /// does not offer Install ([`Self::installs`]) and a ready download
    /// comes with the way by hand instead ([`Self::install_unavailable_of`]).
    pub fn offered(
        platform: Platform,
        dir: PathBuf,
        downloader: Arc<dyn AssetDownloader>,
        installer: Arc<dyn UpdateInstaller>,
    ) -> Self {
        let install_unavailable = if platform.installs_in_app() {
            installer.unavailable()
        } else {
            None
        };
        if let Some(cause) = &install_unavailable {
            tracing::info!(
                cause = %cause,
                "Delta cannot install updates itself here: offering the way by hand"
            );
        }
        Self::with(Availability::Offered(Offered {
            platform,
            dir,
            downloader,
            installer,
            install_unavailable,
        }))
    }

    /// Updates not offered, for `reason`: every download is refused.
    pub fn not_offered(reason: NotOffered) -> Self {
        Self::with(Availability::NotOffered(reason))
    }

    fn with(availability: Availability) -> Self {
        Self {
            availability,
            download: Arc::new(Mutex::new(None)),
            install: Arc::new(Mutex::new(None)),
        }
    }

    /// What the browser may offer next to the notice of `newer` (the release
    /// check's newer release): Update only when this app may update and
    /// `newer` has an asset it would download ([`downloadable_asset`]), so the
    /// offer never leads to a refusal that retrying cannot change.
    pub fn offer(&self, newer: Option<&NewerRelease>) -> UpdateOffer {
        match &self.availability {
            Availability::NotOffered(reason) => reason.offer(),
            Availability::Offered(offered) => {
                match newer.map(|newer| downloadable_asset(&offered.platform, newer)) {
                    Some(Ok(_)) => UpdateOffer::Update,
                    Some(Err(_)) | None => UpdateOffer::None,
                }
            }
        }
    }

    /// Whether this app installs a ready download itself ([`Self::install`]),
    /// so the browser offers Install once the download is ready: where the
    /// platform installs in the app, unless the installer said at startup
    /// that it cannot install here.
    pub fn installs(&self) -> bool {
        self.may_update().is_ok_and(|offered| {
            offered.platform.installs_in_app() && offered.install_unavailable.is_none()
        })
    }

    /// Why this app does not install `newer`'s ready download itself
    /// although its platform installs in the app, and how the user installs
    /// it by hand: only once that download is ready, and only when the
    /// installer said at startup that it cannot install here. `None`
    /// otherwise, including wherever Install is offered.
    pub fn install_unavailable_of(
        &self,
        newer: Option<&NewerRelease>,
    ) -> Option<InstallUnavailable> {
        let offered = self.may_update().ok()?;
        let cause = offered.install_unavailable.as_ref()?;
        match self.download_of(newer)? {
            UpdateDownload::Ready { path, .. } => Some(InstallUnavailable {
                cause: InstallError::Unavailable(cause.clone()).to_string(),
                manual: offered.platform.manual_install(&path),
            }),
            UpdateDownload::Downloading { .. } | UpdateDownload::Failed { .. } => None,
        }
    }

    /// The state of `newer`'s download, if one was asked for. A download of
    /// another release is not `newer`'s, and reads as none.
    pub fn download_of(&self, newer: Option<&NewerRelease>) -> Option<UpdateDownload> {
        let newer = newer?;
        lock(&self.download)
            .clone()
            .filter(|download| download.version() == newer.display_version())
    }

    /// The state of `newer`'s install, if one was asked for and has not been
    /// dismissed. An install of another release is not `newer`'s, and reads
    /// as none.
    pub fn install_of(&self, newer: Option<&NewerRelease>) -> Option<UpdateInstall> {
        let newer = newer?;
        lock(&self.install)
            .clone()
            .filter(|install| install.version() == newer.display_version())
    }

    /// The release the app may restart into, and the installed app that runs
    /// it: the one installed, if an install has ended `Installed`. Refused
    /// otherwise.
    pub fn restart(&self) -> Result<UpdateRestart, UpdateRefusal> {
        match &*lock(&self.install) {
            Some(UpdateInstall::Installed { version, app }) => Ok(UpdateRestart {
                version: version.clone(),
                app: app.clone(),
            }),
            _ => Err(UpdateRefusal::NotInstalled),
        }
    }

    /// What this app updates with, or why it may not update.
    fn may_update(&self) -> Result<&Offered, UpdateRefusal> {
        match &self.availability {
            Availability::NotOffered(reason) => Err(match reason {
                NotOffered::CliLauncher => UpdateRefusal::CliLauncher,
                NotOffered::LocalBuild => UpdateRefusal::LocalBuild,
                NotOffered::NoDownloader => UpdateRefusal::NoDownloader,
            }),
            Availability::Offered(offered) => Ok(offered),
        }
    }
}

fn lock<T>(slot: &Mutex<T>) -> MutexGuard<'_, T> {
    slot.lock().expect("update mutex poisoned")
}

#[cfg(test)]
mod tests {
    use super::testing::{
        asset, linux_update, newer_release, update_on, v060, GatedDownloader, GatedInstaller,
        LINUX_DEB,
    };
    use super::*;

    #[test]
    fn a_build_that_may_not_update_is_refused_before_anything_else() {
        for (reason, offer) in [
            (NotOffered::CliLauncher, UpdateOffer::None),
            (NotOffered::LocalBuild, UpdateOffer::Rebuild),
            (NotOffered::NoDownloader, UpdateOffer::None),
        ] {
            let update = ReleaseUpdate::not_offered(reason);
            assert_eq!(update.offer(Some(&v060())), offer);
            let err = update.start(Some(v060())).unwrap_err();
            let refused_for_reason = match reason {
                NotOffered::CliLauncher => matches!(err, UpdateRefusal::CliLauncher),
                NotOffered::LocalBuild => matches!(err, UpdateRefusal::LocalBuild),
                NotOffered::NoDownloader => matches!(err, UpdateRefusal::NoDownloader),
            };
            assert!(refused_for_reason, "{reason:?}: {err:?}");
        }
    }

    #[test]
    fn update_is_offered_only_for_a_release_this_app_would_download() {
        let downloader = GatedDownloader::new();
        let update = linux_update(&downloader);
        assert_eq!(update.offer(Some(&v060())), UpdateOffer::Update);

        let without_asset = newer_release("0.6.0", vec![asset("Delta_0.6.0_aarch64.dmg")]);
        let mut untrusted = v060();
        untrusted.assets[0].download_url = format!("https://example.com/{LINUX_DEB}");
        let mut undigested = v060();
        undigested.assets[0].digest = None;
        for release in [without_asset, untrusted, undigested] {
            assert_eq!(update.offer(Some(&release)), UpdateOffer::None);
        }

        let elsewhere = update_on(
            Platform::new("windows", "x86_64"),
            &downloader,
            &GatedInstaller::new(),
        );
        assert_eq!(elsewhere.offer(Some(&v060())), UpdateOffer::None);
    }
}
