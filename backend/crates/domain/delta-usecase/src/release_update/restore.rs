//! [`ReleaseUpdate::restore`]: picking up a download an earlier run of the
//! app left behind.

use super::{downloadable_asset, lock, signed_asset, ReleaseUpdate, UpdateDownload};
use crate::ports::DownloadProgress;
use crate::with_causes;
use crate::NewerRelease;

impl ReleaseUpdate {
    /// Pick up the download of `newer` (the release check's newer release)
    /// that an earlier run of the app left in the update directory, so a
    /// restart does not offer Update for a release already downloaded.
    ///
    /// Run once the first check after startup has answered. The file is
    /// verified again against the release's digest and the signature kept
    /// beside it
    /// ([`AssetDownloader::reverify`](crate::AssetDownloader::reverify))
    /// rather than trusted: the user owns the directory. A file that passes
    /// makes the download `Ready`, exactly as if this run had downloaded it.
    /// One that fails, and every file of another release, is removed, and
    /// the download stays as if none had been asked for.
    ///
    /// Does nothing when updates are not offered, when there is no newer
    /// release or none this app would download, when the release carries no
    /// signature of the asset, or when a download has
    /// already been asked for in this run. While the file is checked, the
    /// download reads as `Downloading`, so a request in the meantime joins
    /// the check instead of fetching the release beside it; when the check
    /// finds no file that passes, that request continues as a download
    /// ([`Self::start`]) rather than being dropped.
    pub async fn restore(&self, newer: Option<&NewerRelease>) {
        let Ok(offered) = self.may_update() else {
            return;
        };
        let Some(newer) = newer else {
            return;
        };
        let Ok(asset) = downloadable_asset(&offered.platform, newer) else {
            return;
        };
        let Some(sha256) = asset.sha256() else {
            return;
        };
        let Ok(signed) = signed_asset(newer, asset) else {
            return;
        };
        let version = newer.display_version();
        {
            let mut current = lock(&self.download);
            if current.is_some() {
                return;
            }
            *lock(&self.restoring) = Some(None);
            *current = Some(UpdateDownload::Downloading {
                version: version.clone(),
                progress: DownloadProgress {
                    received: 0,
                    total: None,
                },
            });
        }

        let restored = match offered.downloader.reverify(&signed, &offered.dir).await {
            Ok(Some(path)) => {
                tracing::info!(
                    version = %version,
                    path = %path.display(),
                    "found the newer release of Delta already downloaded, and verified it again"
                );
                Some(UpdateDownload::Ready {
                    version,
                    path,
                    sha256,
                })
            }
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(
                    version = %version,
                    error = %with_causes(&err),
                    "a download of the newer release of Delta left by an earlier run failed its check again: removed it"
                );
                None
            }
        };
        let joined = {
            let mut current = lock(&self.download);
            let joined = lock(&self.restoring).take().flatten();
            let found = restored.is_some();
            *current = restored;
            joined.filter(|_| !found)
        };
        if let Some(joined) = joined {
            tracing::info!(
                version = %joined.display_version(),
                "a download was asked for while the file left behind was checked, and none passed: downloading the release"
            );
            if let Err(err) = self.start(Some(joined)) {
                tracing::warn!(
                    error = %err,
                    "could not download the newer release of Delta asked for while the file left behind was checked"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::testing::{
        asset, linux_update, newer_release, settled, signature_of, test_sha256, v060,
        GatedDownloader, LINUX_DEB,
    };
    use super::*;
    use crate::ports::AssetDownloadError;
    use crate::release_update::{NotOffered, UpdateOffer};

    #[tokio::test]
    async fn a_matching_valid_file_makes_the_download_ready_without_fetching_it() {
        let downloader = GatedDownloader::new();
        downloader.leave_behind(Ok(true));
        let update = linux_update(&downloader);

        update.restore(Some(&v060())).await;

        let ready = UpdateDownload::Ready {
            version: "v0.6.0".into(),
            path: Path::new("/data/updates").join(LINUX_DEB),
            sha256: test_sha256(),
        };
        assert_eq!(update.download_of(Some(&v060())), Some(ready.clone()));
        assert_eq!(downloader.reverified(), [LINUX_DEB]);
        // Asking to download answers the file found, fetching nothing.
        assert_eq!(update.start(Some(v060())).unwrap(), ready);
        assert_eq!(downloader.calls(), 0);
    }

    #[tokio::test]
    async fn a_matching_corrupt_file_leaves_no_download() {
        let downloader = GatedDownloader::new();
        downloader.leave_behind(Err(AssetDownloadError::DigestMismatch {
            expected: test_sha256(),
            actual: "1".repeat(64),
        }));
        let update = linux_update(&downloader);

        update.restore(Some(&v060())).await;

        assert_eq!(update.download_of(Some(&v060())), None);
        assert_eq!(update.offer(Some(&v060())), UpdateOffer::Update);
        // The release is downloaded again on request.
        assert!(matches!(
            update.start(Some(v060())).unwrap(),
            UpdateDownload::Downloading { .. }
        ));
        downloader.finish(Ok(()));
        assert!(matches!(
            settled(&update).await,
            UpdateDownload::Ready { .. }
        ));
        assert_eq!(downloader.calls(), 1);
    }

    /// A request that joined the check is not dropped when no file passes:
    /// it continues as a download of the release.
    #[tokio::test]
    async fn a_request_during_a_check_that_finds_no_file_downloads_the_release() {
        let corrupt = || {
            Err(AssetDownloadError::DigestMismatch {
                expected: test_sha256(),
                actual: "1".repeat(64),
            })
        };
        for left_behind in [corrupt(), Ok(false)] {
            let downloader = GatedDownloader::new();
            downloader.leave_behind(left_behind);
            let release = downloader.hold_reverify();
            let update = linux_update(&downloader);

            let newer = v060();
            tokio::join!(update.restore(Some(&newer)), async {
                while downloader.reverified().is_empty() {
                    tokio::task::yield_now().await;
                }
                // Joins the check: nothing is fetched beside it.
                assert!(matches!(
                    update.start(Some(v060())).unwrap(),
                    UpdateDownload::Downloading { .. }
                ));
                assert_eq!(downloader.calls(), 0);
                release.send(()).unwrap();
            });

            assert!(matches!(
                update.download_of(Some(&v060())),
                Some(UpdateDownload::Downloading { .. })
            ));
            downloader.finish(Ok(()));
            assert!(matches!(
                settled(&update).await,
                UpdateDownload::Ready { .. }
            ));
            assert_eq!(downloader.calls(), 1);
        }
    }

    /// A request that joined a check finding a file that passes is answered
    /// by that file, with nothing fetched.
    #[tokio::test]
    async fn a_request_during_a_check_that_finds_the_file_fetches_nothing() {
        let downloader = GatedDownloader::new();
        downloader.leave_behind(Ok(true));
        let release = downloader.hold_reverify();
        let update = linux_update(&downloader);

        let newer = v060();
        tokio::join!(update.restore(Some(&newer)), async {
            while downloader.reverified().is_empty() {
                tokio::task::yield_now().await;
            }
            update.start(Some(v060())).unwrap();
            release.send(()).unwrap();
        });

        assert!(matches!(
            update.download_of(Some(&v060())),
            Some(UpdateDownload::Ready { .. })
        ));
        tokio::task::yield_now().await;
        assert_eq!(downloader.calls(), 0);
    }

    #[tokio::test]
    async fn a_file_of_an_older_release_is_not_the_newer_releases() {
        let downloader = GatedDownloader::new();
        // Only v0.6.0's file is there; the gateway finds none of v0.7.0's
        // (and removes v0.6.0's).
        downloader.leave_behind(Ok(false));
        let update = linux_update(&downloader);
        let v070 = newer_release(
            "0.7.0",
            vec![
                asset("delta-desktop_0.7.0_amd64.deb"),
                signature_of("delta-desktop_0.7.0_amd64.deb"),
            ],
        );

        update.restore(Some(&v070)).await;

        assert_eq!(downloader.reverified(), ["delta-desktop_0.7.0_amd64.deb"]);
        assert_eq!(update.download_of(Some(&v070)), None);
        assert_eq!(update.download_of(Some(&v060())), None);
    }

    #[tokio::test]
    async fn no_file_leaves_no_download() {
        let downloader = GatedDownloader::new();
        downloader.leave_behind(Ok(false));
        let update = linux_update(&downloader);

        update.restore(Some(&v060())).await;

        assert_eq!(update.download_of(Some(&v060())), None);
        assert_eq!(update.offer(Some(&v060())), UpdateOffer::Update);
    }

    #[tokio::test]
    async fn nothing_is_looked_for_without_a_release_to_download_or_once_one_was_asked_for() {
        let downloader = GatedDownloader::new();
        let update = linux_update(&downloader);
        update.restore(None).await;
        let mut undigested = v060();
        undigested.assets[0].digest = None;
        update.restore(Some(&undigested)).await;

        let unsigned = newer_release("0.6.0", vec![asset(LINUX_DEB)]);
        update.restore(Some(&unsigned)).await;
        assert_eq!(update.download_of(Some(&unsigned)), None);

        update.start(Some(v060())).unwrap();
        update.restore(Some(&v060())).await;
        assert!(downloader.reverified().is_empty());
        downloader.finish(Ok(()));
        settled(&update).await;

        let not_offered = ReleaseUpdate::not_offered(NotOffered::LocalBuild);
        not_offered.restore(Some(&v060())).await;
        assert_eq!(not_offered.download_of(Some(&v060())), None);
        assert!(downloader.reverified().is_empty());
    }
}
