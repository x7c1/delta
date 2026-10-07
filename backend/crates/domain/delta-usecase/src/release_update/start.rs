//! [`ReleaseUpdate::start`]: downloading the newer release in the background.

use std::sync::Arc;

use super::{
    downloadable_asset, lock, ReleaseUpdate, UpdateDownload, UpdateInstall, UpdateRefusal,
};
use crate::ports::DownloadProgress;
use crate::release_check::with_causes;
use crate::NewerRelease;

impl ReleaseUpdate {
    /// Start downloading `newer` (the release check's newer release), or
    /// report the download already running or done.
    ///
    /// Refused, with nothing fetched, when updates are not offered, when there
    /// is no newer release, or when it has no asset this platform may download
    /// ([`downloadable_asset`]). Otherwise answers the download's
    /// state: the one running (whichever release it is of), `Ready` when this
    /// release is already downloaded, or a fresh `Downloading` whose transfer
    /// now runs in the background (a failed download starts over). A fresh
    /// download clears an install whose file was rejected
    /// ([`UpdateInstall::Rejected`]), so the new file is offered for install.
    pub fn start(&self, newer: Option<NewerRelease>) -> Result<UpdateDownload, UpdateRefusal> {
        let offered = self.may_update()?;
        let newer = newer.ok_or(UpdateRefusal::NoNewerRelease)?;
        let version = newer.display_version();

        let mut current = lock(&self.download);
        match &*current {
            Some(running @ UpdateDownload::Downloading { .. }) => return Ok(running.clone()),
            Some(ready @ UpdateDownload::Ready { version: done, .. }) if *done == version => {
                return Ok(ready.clone())
            }
            _ => {}
        }
        let asset = downloadable_asset(&offered.platform, &newer)?.clone();

        let started = UpdateDownload::Downloading {
            version: version.clone(),
            progress: DownloadProgress {
                received: 0,
                total: None,
            },
        };
        *current = Some(started.clone());
        drop(current);
        let mut install = lock(&self.install);
        if matches!(&*install, Some(UpdateInstall::Rejected { .. })) {
            *install = None;
        }
        drop(install);

        tracing::info!(
            version = %version,
            asset = %asset.name,
            url = %asset.download_url,
            "downloading the newer release of Delta"
        );
        let slot = Arc::clone(&self.download);
        let downloader = Arc::clone(&offered.downloader);
        let dir = offered.dir.clone();
        tokio::spawn(async move {
            let progress_slot = Arc::clone(&slot);
            let report = move |progress: DownloadProgress| {
                let mut current = lock(&progress_slot);
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
            *lock(&slot) = Some(done);
        });
        Ok(started)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::testing::{
        asset, linux_update, newer_release, settled, v060, GatedDownloader, LINUX_DEB,
    };
    use super::*;
    use crate::ports::AssetDownloadError;
    use crate::release_update::UpdateOffer;

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
