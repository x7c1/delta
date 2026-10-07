//! Downloading the newer release the release check found.
//!
//! [`ReleaseUpdate`] is the first step of an in-app update: it downloads this
//! platform's asset of the newer release into the update directory and
//! verifies its sha256. It does not apply anything; replacing the app and
//! restarting it are later steps.
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
//! # One download at a time
//!
//! The transfer runs in the background and its state ([`UpdateDownload`]) is
//! held here for the browser to read. A request while one runs joins it rather
//! than starting a second; a request after a failure starts over; a request
//! after the same release is ready answers ready without downloading again.

mod asset_choice;
pub use asset_choice::{
    check_download_url, choose_asset, downloadable_asset, RELEASE_DOWNLOAD_PREFIX,
};
mod build_origin;
pub use build_origin::BuildOrigin;
mod launcher;
pub use launcher::Launcher;
mod not_offered;
pub use not_offered::NotOffered;
mod platform;
pub use platform::Platform;
mod update_download;
pub use update_download::UpdateDownload;
mod update_offer;
pub use update_offer::UpdateOffer;
mod update_refusal;
pub use update_refusal::UpdateRefusal;

#[cfg(test)]
mod testing;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::ports::{AssetDownloader, DownloadProgress};
use crate::release_check::with_causes;
use crate::NewerRelease;

/// The update download: whether it is offered, and the state of the last one.
pub struct ReleaseUpdate {
    availability: Availability,
    download: Arc<Mutex<Option<UpdateDownload>>>,
}

enum Availability {
    NotOffered(NotOffered),
    Offered {
        platform: Platform,
        dir: PathBuf,
        downloader: Arc<dyn AssetDownloader>,
    },
}

impl ReleaseUpdate {
    /// Updates offered: `platform`'s asset is downloaded into `dir` (the data
    /// directory's `updates/`) through `downloader`.
    pub fn offered(platform: Platform, dir: PathBuf, downloader: Arc<dyn AssetDownloader>) -> Self {
        Self::with(Availability::Offered {
            platform,
            dir,
            downloader,
        })
    }

    /// Updates not offered, for `reason`: every download is refused.
    pub fn not_offered(reason: NotOffered) -> Self {
        Self::with(Availability::NotOffered(reason))
    }

    fn with(availability: Availability) -> Self {
        Self {
            availability,
            download: Arc::new(Mutex::new(None)),
        }
    }

    /// What the browser may offer next to the notice of `newer` (the release
    /// check's newer release): Update only when this app may update and
    /// `newer` has an asset it would download ([`downloadable_asset`]), so the
    /// offer never leads to a refusal that retrying cannot change.
    pub fn offer(&self, newer: Option<&NewerRelease>) -> UpdateOffer {
        match &self.availability {
            Availability::NotOffered(reason) => reason.offer(),
            Availability::Offered { platform, .. } => {
                match newer.map(|newer| downloadable_asset(platform, newer)) {
                    Some(Ok(_)) => UpdateOffer::Update,
                    Some(Err(_)) | None => UpdateOffer::None,
                }
            }
        }
    }

    /// The state of `newer`'s download, if one was asked for. A download of
    /// another release is not `newer`'s, and reads as none.
    pub fn download_of(&self, newer: Option<&NewerRelease>) -> Option<UpdateDownload> {
        let newer = newer?;
        self.lock()
            .clone()
            .filter(|download| download.version() == newer.display_version())
    }

    /// Start downloading `newer` (the release check's newer release), or
    /// report the download already running or done.
    ///
    /// Refused, with nothing fetched, when updates are not offered, when there
    /// is no newer release, or when it has no asset this platform may download
    /// ([`downloadable_asset`]). Otherwise answers the download's
    /// state: the one running (whichever release it is of), `Ready` when this
    /// release is already downloaded, or a fresh `Downloading` whose transfer
    /// now runs in the background (a failed download starts over).
    pub fn start(&self, newer: Option<NewerRelease>) -> Result<UpdateDownload, UpdateRefusal> {
        let (platform, dir, downloader) = match &self.availability {
            Availability::NotOffered(reason) => {
                return Err(match reason {
                    NotOffered::CliLauncher => UpdateRefusal::CliLauncher,
                    NotOffered::LocalBuild => UpdateRefusal::LocalBuild,
                    NotOffered::NoDownloader => UpdateRefusal::NoDownloader,
                })
            }
            Availability::Offered {
                platform,
                dir,
                downloader,
            } => (platform, dir, downloader),
        };
        let newer = newer.ok_or(UpdateRefusal::NoNewerRelease)?;
        let version = newer.display_version();

        let mut current = self.lock();
        match &*current {
            Some(running @ UpdateDownload::Downloading { .. }) => return Ok(running.clone()),
            Some(ready @ UpdateDownload::Ready { version: done, .. }) if *done == version => {
                return Ok(ready.clone())
            }
            _ => {}
        }
        let asset = downloadable_asset(platform, &newer)?.clone();

        let started = UpdateDownload::Downloading {
            version: version.clone(),
            progress: DownloadProgress {
                received: 0,
                total: None,
            },
        };
        *current = Some(started.clone());
        drop(current);

        tracing::info!(
            version = %version,
            asset = %asset.name,
            url = %asset.download_url,
            "downloading the newer release of Delta"
        );
        let slot = Arc::clone(&self.download);
        let downloader = Arc::clone(downloader);
        let dir = dir.clone();
        tokio::spawn(async move {
            let progress_slot = Arc::clone(&slot);
            let report = move |progress: DownloadProgress| {
                let mut current = progress_slot.lock().expect("update mutex poisoned");
                if let Some(UpdateDownload::Downloading { progress: held, .. }) = &mut *current {
                    *held = progress;
                }
            };
            let done = match downloader.download(&asset, &dir, &report).await {
                Ok(path) => {
                    tracing::info!(
                        version = %version,
                        path = %path.display(),
                        "downloaded and verified the newer release of Delta"
                    );
                    UpdateDownload::Ready { version, path }
                }
                Err(err) => {
                    let cause = with_causes(&err);
                    tracing::warn!(
                        version = %version,
                        url = %asset.download_url,
                        error = %cause,
                        "could not download the newer release of Delta"
                    );
                    UpdateDownload::Failed { version, cause }
                }
            };
            *slot.lock().expect("update mutex poisoned") = Some(done);
        });
        Ok(started)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<UpdateDownload>> {
        self.download.lock().expect("update mutex poisoned")
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::testing::{asset, newer_release, GatedDownloader};
    use super::*;
    use crate::ports::AssetDownloadError;

    const LINUX_DEB: &str = "delta-desktop_0.6.0_amd64.deb";

    fn linux_update(downloader: &Arc<GatedDownloader>) -> ReleaseUpdate {
        ReleaseUpdate::offered(
            Platform::new("linux", "x86_64"),
            PathBuf::from("/data/updates"),
            Arc::clone(downloader) as Arc<dyn AssetDownloader>,
        )
    }

    fn v060() -> NewerRelease {
        newer_release("0.6.0", vec![asset(LINUX_DEB)])
    }

    /// Wait until the background transfer has recorded its outcome.
    async fn settled(update: &ReleaseUpdate) -> UpdateDownload {
        for _ in 0..200 {
            match update.lock().clone() {
                Some(UpdateDownload::Downloading { .. }) | None => {}
                Some(done) => return done,
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("the download never settled");
    }

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

    #[tokio::test]
    async fn no_newer_release_is_refused() {
        let downloader = GatedDownloader::new();
        let update = linux_update(&downloader);
        assert_eq!(update.offer(None), UpdateOffer::None);
        assert!(matches!(
            update.start(None),
            Err(UpdateRefusal::NoNewerRelease)
        ));
        assert_eq!(downloader.calls(), 0);
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

        let elsewhere = ReleaseUpdate::offered(
            Platform::new("windows", "x86_64"),
            PathBuf::from("/data/updates"),
            Arc::clone(&downloader) as Arc<dyn AssetDownloader>,
        );
        assert_eq!(elsewhere.offer(Some(&v060())), UpdateOffer::None);
    }

    #[tokio::test]
    async fn an_asset_outside_the_pinned_prefix_is_refused_before_any_request() {
        let downloader = GatedDownloader::new();
        let update = linux_update(&downloader);
        let mut release = v060();
        release.assets[0].download_url = format!("https://example.com/{LINUX_DEB}");
        assert!(matches!(
            update.start(Some(release)),
            Err(UpdateRefusal::UntrustedUrl(_))
        ));
        tokio::task::yield_now().await;
        assert_eq!(downloader.calls(), 0);
        assert_eq!(update.download_of(Some(&v060())), None);
    }

    #[tokio::test]
    async fn a_download_runs_once_and_ends_ready() {
        let downloader = GatedDownloader::new();
        let update = linux_update(&downloader);

        let started = update.start(Some(v060())).unwrap();
        assert!(matches!(started, UpdateDownload::Downloading { .. }));
        // A second request while the first runs joins it.
        assert!(matches!(
            update.start(Some(v060())).unwrap(),
            UpdateDownload::Downloading { .. }
        ));
        downloader.finish(Ok(()));
        let done = settled(&update).await;
        assert_eq!(
            done,
            UpdateDownload::Ready {
                version: "v0.6.0".into(),
                path: Path::new("/data/updates").join(LINUX_DEB),
            }
        );
        // Ready for this release: answered without downloading again.
        assert_eq!(update.start(Some(v060())).unwrap(), done);
        assert_eq!(downloader.calls(), 1);
        assert_eq!(update.download_of(Some(&v060())), Some(done));
        // Not the download of another release.
        let v070 = newer_release("0.7.0", vec![asset("delta-desktop_0.7.0_amd64.deb")]);
        assert_eq!(update.download_of(Some(&v070)), None);
    }

    #[tokio::test]
    async fn a_failed_download_reports_its_cause_and_a_retry_starts_over() {
        let downloader = GatedDownloader::new();
        let update = linux_update(&downloader);

        update.start(Some(v060())).unwrap();
        downloader.finish(Err(AssetDownloadError::Status(404)));
        assert_eq!(
            settled(&update).await,
            UpdateDownload::Failed {
                version: "v0.6.0".into(),
                cause: "the download answered with HTTP status 404".into(),
            }
        );

        assert!(matches!(
            update.start(Some(v060())).unwrap(),
            UpdateDownload::Downloading { .. }
        ));
        downloader.finish(Ok(()));
        assert!(matches!(
            settled(&update).await,
            UpdateDownload::Ready { .. }
        ));
        assert_eq!(downloader.calls(), 2);
    }

    #[tokio::test]
    async fn progress_is_reported_while_the_transfer_runs() {
        let downloader = GatedDownloader::new();
        let update = linux_update(&downloader);
        update.start(Some(v060())).unwrap();
        downloader.wait_started().await;
        assert_eq!(
            update.download_of(Some(&v060())),
            Some(UpdateDownload::Downloading {
                version: "v0.6.0".into(),
                progress: GatedDownloader::PROGRESS,
            })
        );
        downloader.finish(Ok(()));
        settled(&update).await;
    }
}
