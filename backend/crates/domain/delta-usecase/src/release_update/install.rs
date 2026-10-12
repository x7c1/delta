//! [`ReleaseUpdate::install`]: installing a verified download in the background.

use std::io::ErrorKind;
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::{lock, ReleaseUpdate, UpdateDownload, UpdateInstall, UpdateRefusal};
use crate::ports::InstallError;
use crate::NewerRelease;

impl ReleaseUpdate {
    /// Start installing the verified download of `newer`, or report the
    /// install already running or done.
    ///
    /// Refused, with nothing run, when updates are not offered, when this
    /// platform does not install in the app, when there is no newer release,
    /// or when no verified download of it is ready. Otherwise answers the
    /// install's state: the one running, `Installed` when this release is
    /// already installed, or a fresh `Installing` whose install now runs in
    /// the background (after a failure, or when Delta could not install,
    /// it tries again).
    ///
    /// The install ends `Installed`, `Rejected`, `Failed` or `Unavailable`;
    /// a dismissed password prompt clears the state, so the download reads as
    /// ready again. When the installer rejects the file itself, the file is
    /// removed and the download cleared, so the next step is downloading the
    /// release again ([`Self::start`]), never installing that file.
    pub fn install(&self, newer: Option<NewerRelease>) -> Result<UpdateInstall, UpdateRefusal> {
        let offered = self.may_update()?;
        if !offered.platform.installs_in_app() {
            return Err(UpdateRefusal::InstallUnsupported {
                platform: offered.platform.to_string(),
            });
        }
        let newer = newer.ok_or(UpdateRefusal::NoNewerRelease)?;
        let version = newer.display_version();
        let (path, sha256) = match self.download_of(Some(&newer)) {
            Some(UpdateDownload::Ready { path, sha256, .. }) => (path, sha256),
            _ => return Err(UpdateRefusal::NotReady),
        };

        let mut current = lock(&self.install);
        match &*current {
            Some(running @ UpdateInstall::Installing { .. }) => return Ok(running.clone()),
            Some(
                done @ UpdateInstall::Installed {
                    version: installed, ..
                },
            ) if *installed == version => return Ok(done.clone()),
            _ => {}
        }
        let started = UpdateInstall::Installing {
            version: version.clone(),
        };
        *current = Some(started.clone());
        drop(current);

        tracing::info!(
            version = %version,
            path = %path.display(),
            "installing the newer release of Delta"
        );
        let slot = Arc::clone(&self.install);
        let download = Arc::clone(&self.download);
        let installer = Arc::clone(&offered.installer);
        let manual = offered.platform.manual_install(&path);
        tokio::spawn(async move {
            let done = match installer.install(&version, &path, &sha256).await {
                Ok(app) => {
                    tracing::info!(
                        version = %version,
                        app = ?app,
                        "installed the newer release of Delta"
                    );
                    Some(UpdateInstall::Installed { version, app })
                }
                Err(InstallError::Dismissed) => {
                    tracing::info!(
                        version = %version,
                        "the install of the newer release of Delta was dismissed"
                    );
                    None
                }
                Err(err @ (InstallError::Unavailable(_) | InstallError::Unrunnable { .. })) => {
                    tracing::warn!(
                        version = %version,
                        error = %err,
                        manual = ?manual,
                        "Delta cannot install the newer release itself"
                    );
                    Some(UpdateInstall::Unavailable {
                        version,
                        cause: err.to_string(),
                        manual,
                    })
                }
                Err(InstallError::Rejected(cause)) => {
                    tracing::warn!(
                        version = %version,
                        path = %path.display(),
                        error = %cause,
                        "the newer release of Delta was rejected: removing its download"
                    );
                    discard(&download, &path);
                    Some(UpdateInstall::Rejected { version, cause })
                }
                Err(InstallError::Failed(cause)) => {
                    tracing::warn!(
                        version = %version,
                        error = %cause,
                        manual = ?manual,
                        "could not install the newer release of Delta"
                    );
                    Some(UpdateInstall::Failed {
                        version,
                        cause,
                        manual,
                    })
                }
            };
            *lock(&slot) = done;
        });
        Ok(started)
    }
}

/// Remove the rejected download at `path`, and the `Ready` state that
/// points at it, so it is neither offered for install again nor left in the
/// update directory. A file that cannot be removed is logged; the state is
/// cleared regardless.
fn discard(download: &Mutex<Option<UpdateDownload>>, path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(
            path = %path.display(),
            error = %err,
            "could not remove the rejected download of the newer release of Delta"
        ),
    }
    let mut current = lock(download);
    if matches!(&*current, Some(UpdateDownload::Ready { path: ready, .. }) if ready == path) {
        *current = None;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::testing::{
        asset, installed_app, newer_release, settled, test_sha256, update_on, v060,
        GatedDownloader, GatedInstaller, LINUX_DEB, MAC_DMG,
    };
    use super::*;
    use crate::release_update::{InstallUnavailable, ManualInstall, NotOffered, Platform};

    /// A Linux update whose download of v0.6.0 is ready, installing through
    /// `installer`.
    async fn ready_linux_update(installer: &Arc<GatedInstaller>) -> ReleaseUpdate {
        let downloader = GatedDownloader::new();
        let update = update_on(Platform::new("linux", "x86_64"), &downloader, installer);
        update.start(Some(v060())).unwrap();
        downloader.finish(Ok(()));
        settled(&update).await;
        update
    }

    /// Wait until the background install has recorded its outcome (`None`
    /// once dismissed).
    async fn install_settled(update: &ReleaseUpdate) -> Option<UpdateInstall> {
        for _ in 0..200 {
            match lock(&update.install).clone() {
                Some(UpdateInstall::Installing { .. }) => {}
                done => return done,
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("the install never settled");
    }

    fn manual() -> ManualInstall {
        ManualInstall::Command(
            "sudo apt install /data/updates/delta-desktop_0.6.0_amd64.deb".into(),
        )
    }

    #[tokio::test]
    async fn an_install_is_refused_unless_a_verified_download_is_ready() {
        for reason in [
            NotOffered::CliLauncher,
            NotOffered::LocalBuild,
            NotOffered::NoDownloader,
        ] {
            let update = ReleaseUpdate::not_offered(reason);
            assert!(!update.installs());
            assert!(update.install(Some(v060())).is_err());
        }

        let installer = GatedInstaller::new();
        let downloader = GatedDownloader::new();
        let elsewhere = update_on(Platform::new("windows", "x86_64"), &downloader, &installer);
        assert!(!elsewhere.installs());
        assert!(matches!(
            elsewhere.install(Some(v060())),
            Err(UpdateRefusal::InstallUnsupported { .. })
        ));

        let linux = update_on(Platform::new("linux", "x86_64"), &downloader, &installer);
        assert!(linux.installs());
        assert!(matches!(
            linux.install(None),
            Err(UpdateRefusal::NoNewerRelease)
        ));
        // Not downloaded yet, then downloading.
        assert!(matches!(
            linux.install(Some(v060())),
            Err(UpdateRefusal::NotReady)
        ));
        linux.start(Some(v060())).unwrap();
        assert!(matches!(
            linux.install(Some(v060())),
            Err(UpdateRefusal::NotReady)
        ));
        downloader.finish(Ok(()));
        settled(&linux).await;
        // Ready, but for another release than the newer one.
        let v070 = newer_release("0.7.0", vec![asset("delta-desktop_0.7.0_amd64.deb")]);
        assert!(matches!(
            linux.install(Some(v070)),
            Err(UpdateRefusal::NotReady)
        ));
        tokio::task::yield_now().await;
        assert!(installer.asked().is_empty());
    }

    #[tokio::test]
    async fn an_install_runs_once_and_ends_installed() {
        let installer = GatedInstaller::new();
        let update = ready_linux_update(&installer).await;
        assert!(matches!(update.restart(), Err(UpdateRefusal::NotInstalled)));

        let installing = UpdateInstall::Installing {
            version: "v0.6.0".into(),
        };
        assert_eq!(update.install(Some(v060())).unwrap(), installing);
        // A second request while the first runs starts nothing.
        assert_eq!(update.install(Some(v060())).unwrap(), installing);
        assert_eq!(update.install_of(Some(&v060())), Some(installing));
        assert!(matches!(update.restart(), Err(UpdateRefusal::NotInstalled)));

        installer.finish(Ok(()));
        let installed = UpdateInstall::Installed {
            version: "v0.6.0".into(),
            app: installed_app(),
        };
        assert_eq!(install_settled(&update).await, Some(installed.clone()));
        assert_eq!(update.install(Some(v060())).unwrap(), installed);
        assert_eq!(
            installer.asked(),
            vec![(
                "v0.6.0".to_owned(),
                Path::new("/data/updates").join(LINUX_DEB),
                test_sha256(),
            )]
        );
        let restart = update.restart().unwrap();
        assert_eq!(restart.version, "v0.6.0");
        assert_eq!(restart.app, installed_app());
    }

    #[tokio::test]
    async fn a_dismissed_install_leaves_the_download_ready() {
        let installer = GatedInstaller::new();
        let update = ready_linux_update(&installer).await;
        update.install(Some(v060())).unwrap();
        installer.finish(Err(InstallError::Dismissed));
        assert_eq!(install_settled(&update).await, None);
        assert_eq!(update.install_of(Some(&v060())), None);
        assert!(matches!(
            update.download_of(Some(&v060())),
            Some(UpdateDownload::Ready { .. })
        ));
    }

    #[tokio::test]
    async fn an_install_delta_cannot_run_carries_the_manual_command() {
        let installer = GatedInstaller::new();
        let update = ready_linux_update(&installer).await;
        update.install(Some(v060())).unwrap();
        installer.finish(Err(InstallError::Unavailable(
            "no authentication agent".into(),
        )));
        assert_eq!(
            install_settled(&update).await,
            Some(UpdateInstall::Unavailable {
                version: "v0.6.0".into(),
                cause: "Delta cannot install the update itself: no authentication agent".into(),
                manual: manual(),
            })
        );
        assert!(matches!(update.restart(), Err(UpdateRefusal::NotInstalled)));

        // A new request tries again.
        assert!(matches!(
            update.install(Some(v060())).unwrap(),
            UpdateInstall::Installing { .. }
        ));
        installer.finish(Ok(()));
        assert!(matches!(
            install_settled(&update).await,
            Some(UpdateInstall::Installed { .. })
        ));
        assert_eq!(installer.asked().len(), 2);
    }

    #[tokio::test]
    async fn a_rejected_file_is_removed_and_the_release_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        let downloader = GatedDownloader::new();
        let installer = GatedInstaller::new();
        let update = ReleaseUpdate::offered(
            Platform::new("linux", "x86_64"),
            dir.path().to_path_buf(),
            Arc::clone(&downloader) as Arc<dyn crate::ports::AssetDownloader>,
            Arc::clone(&installer) as Arc<dyn crate::ports::UpdateInstaller>,
        );
        update.start(Some(v060())).unwrap();
        downloader.finish(Ok(()));
        settled(&update).await;
        let file = dir.path().join(LINUX_DEB);
        std::fs::write(&file, b"not the release").unwrap();

        update.install(Some(v060())).unwrap();
        installer.finish(Err(InstallError::Rejected(
            "the update file's sha256 is 00, not release v0.6.0's ff".into(),
        )));
        let rejected = UpdateInstall::Rejected {
            version: "v0.6.0".into(),
            cause: "the update file's sha256 is 00, not release v0.6.0's ff".into(),
        };
        assert_eq!(install_settled(&update).await, Some(rejected.clone()));
        assert!(!file.exists());
        assert_eq!(update.download_of(Some(&v060())), None);
        // Not installable again: the file is gone.
        assert!(matches!(
            update.install(Some(v060())),
            Err(UpdateRefusal::NotReady)
        ));
        assert_eq!(update.install_of(Some(&v060())), Some(rejected));

        // Downloading again clears the rejection and leads to Install again.
        update.start(Some(v060())).unwrap();
        assert_eq!(update.install_of(Some(&v060())), None);
        downloader.finish(Ok(()));
        settled(&update).await;
        assert!(matches!(
            update.install(Some(v060())).unwrap(),
            UpdateInstall::Installing { .. }
        ));
        installer.finish(Ok(()));
        assert!(matches!(
            install_settled(&update).await,
            Some(UpdateInstall::Installed { .. })
        ));
    }

    /// The installer also succeeds when the installed app is that version or
    /// newer already (installed from a terminal, say): the update reads as
    /// installed, and its file is kept, unlike a rejected one.
    #[tokio::test]
    async fn an_installed_update_keeps_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let downloader = GatedDownloader::new();
        let installer = GatedInstaller::new();
        let update = ReleaseUpdate::offered(
            Platform::new("linux", "x86_64"),
            dir.path().to_path_buf(),
            Arc::clone(&downloader) as Arc<dyn crate::ports::AssetDownloader>,
            Arc::clone(&installer) as Arc<dyn crate::ports::UpdateInstaller>,
        );
        update.start(Some(v060())).unwrap();
        downloader.finish(Ok(()));
        settled(&update).await;
        let file = dir.path().join(LINUX_DEB);
        std::fs::write(&file, b"the release").unwrap();

        update.install(Some(v060())).unwrap();
        installer.finish(Ok(()));
        assert_eq!(
            install_settled(&update).await,
            Some(UpdateInstall::Installed {
                version: "v0.6.0".into(),
                app: installed_app(),
            })
        );
        assert!(file.exists());
        assert!(matches!(
            update.download_of(Some(&v060())),
            Some(UpdateDownload::Ready { .. })
        ));
        assert_eq!(update.restart().unwrap().version, "v0.6.0");
    }

    #[tokio::test]
    async fn a_failed_install_carries_the_reason_and_the_manual_command() {
        let installer = GatedInstaller::new();
        let update = ready_linux_update(&installer).await;
        update.install(Some(v060())).unwrap();
        installer.finish(Err(InstallError::Failed(
            "apt-get could not install the update: E: broken".into(),
        )));
        assert_eq!(
            install_settled(&update).await,
            Some(UpdateInstall::Failed {
                version: "v0.6.0".into(),
                cause: "apt-get could not install the update: E: broken".into(),
                manual: manual(),
            })
        );
    }

    /// On macOS the app installs the downloaded disk image itself, and the
    /// way by hand is opening that image, not a terminal command.
    #[tokio::test]
    async fn a_mac_install_offers_the_disk_image_by_hand() {
        let downloader = GatedDownloader::new();
        let installer = GatedInstaller::new();
        let mac = update_on(Platform::new("macos", "aarch64"), &downloader, &installer);
        assert!(mac.installs());
        let v060_mac = || newer_release("0.6.0", vec![asset(MAC_DMG)]);
        mac.start(Some(v060_mac())).unwrap();
        downloader.finish(Ok(()));
        settled(&mac).await;

        mac.install(Some(v060_mac())).unwrap();
        installer.finish(Err(InstallError::Unavailable(
            "Delta runs from a translocated copy".into(),
        )));
        let image = Path::new("/data/updates").join(MAC_DMG);
        assert_eq!(
            install_settled(&mac).await,
            Some(UpdateInstall::Unavailable {
                version: "v0.6.0".into(),
                cause:
                    "Delta cannot install the update itself: Delta runs from a translocated copy"
                        .into(),
                manual: ManualInstall::DiskImage(image.clone()),
            })
        );

        mac.install(Some(v060_mac())).unwrap();
        installer.finish(Err(InstallError::Failed("hdiutil could not attach".into())));
        assert_eq!(
            install_settled(&mac).await,
            Some(UpdateInstall::Failed {
                version: "v0.6.0".into(),
                cause: "hdiutil could not attach".into(),
                manual: ManualInstall::DiskImage(image.clone()),
            })
        );
        assert_eq!(
            installer.asked()[0],
            ("v0.6.0".to_owned(), image, test_sha256())
        );
        assert_eq!(mac.install_unavailable_of(Some(&v060_mac())), None);
    }

    /// An installer that says at startup it cannot install here (on macOS,
    /// a translocated or unwritable bundle) turns Install off: a ready
    /// download comes with the reason and the disk image to open instead.
    #[tokio::test]
    async fn an_installer_unavailable_at_startup_offers_the_way_by_hand_once_ready() {
        let downloader = GatedDownloader::new();
        let installer = GatedInstaller::unavailable_because("Delta runs from a translocated copy");
        let mac = update_on(Platform::new("macos", "aarch64"), &downloader, &installer);
        let v060_mac = || newer_release("0.6.0", vec![asset(MAC_DMG)]);
        assert!(!mac.installs());
        assert_eq!(mac.offer(Some(&v060_mac())), crate::UpdateOffer::Update);
        // Nothing to install by hand before the download is ready.
        assert_eq!(mac.install_unavailable_of(Some(&v060_mac())), None);
        mac.start(Some(v060_mac())).unwrap();
        assert_eq!(mac.install_unavailable_of(Some(&v060_mac())), None);
        downloader.finish(Ok(()));
        settled(&mac).await;

        assert_eq!(
            mac.install_unavailable_of(Some(&v060_mac())),
            Some(InstallUnavailable {
                cause:
                    "Delta cannot install the update itself: Delta runs from a translocated copy"
                        .into(),
                manual: ManualInstall::DiskImage(Path::new("/data/updates").join(MAC_DMG)),
            })
        );
        assert_eq!(mac.install_unavailable_of(None), None);

        // Where the platform installs nothing in the app, there is nothing
        // to turn off, whatever the installer says.
        let elsewhere = update_on(Platform::new("windows", "x86_64"), &downloader, &installer);
        assert!(!elsewhere.installs());
        assert_eq!(elsewhere.install_unavailable_of(Some(&v060())), None);
    }
}
