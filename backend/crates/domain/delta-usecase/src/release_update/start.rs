//! [`ReleaseUpdate::start`]: downloading the newer release in the background.

use std::sync::Arc;

use super::{
    downloadable_asset, lock, signed_asset, ReleaseUpdate, UpdateDownload, UpdateInstall,
    UpdateRefusal,
};
use crate::ports::{AssetDownloadError, DownloadProgress};
use crate::with_causes;
use crate::NewerRelease;

impl ReleaseUpdate {
    /// Start downloading `newer` (the release check's newer release), or
    /// report the download already running or done.
    ///
    /// Refused, with nothing fetched, when updates are not offered, when there
    /// is no newer release, or when it has no asset this platform may download
    /// ([`downloadable_asset`]). Otherwise answers the download's
    /// state: the one running (whichever release it is of), `Ready` when this
    /// release is already downloaded, `Unavailable` when it was found not to
    /// verify, or a fresh `Downloading` whose transfer now runs in the
    /// background (a failed download starts over). A release that carries
    /// no signature of the asset ([`signed_asset`]) is `Unavailable` at once,
    /// with nothing fetched. A request that joins [`Self::restore`]'s check
    /// of a file left behind is remembered, and continues as a download if
    /// the check finds none. A fresh download clears an install whose file
    /// was rejected ([`UpdateInstall::Rejected`]), so the new file is offered
    /// for install.
    pub fn start(&self, newer: Option<NewerRelease>) -> Result<UpdateDownload, UpdateRefusal> {
        let offered = self.may_update()?;
        let newer = newer.ok_or(UpdateRefusal::NoNewerRelease)?;
        let version = newer.display_version();

        let mut current = lock(&self.download);
        match &*current {
            Some(running @ UpdateDownload::Downloading { .. }) => {
                if let Some(joined) = &mut *lock(&self.restoring) {
                    *joined = Some(newer);
                }
                return Ok(running.clone());
            }
            Some(
                done @ (UpdateDownload::Ready { version: of, .. }
                | UpdateDownload::Unavailable { version: of, .. }),
            ) if *of == version => return Ok(done.clone()),
            _ => {}
        }
        let asset = downloadable_asset(&offered.platform, &newer)?;
        // `downloadable_asset` accepts only an asset stating a sha256.
        let sha256 = asset.sha256().ok_or_else(|| UpdateRefusal::NoDigest {
            version: version.clone(),
            asset: asset.name.clone(),
        })?;
        let signed = match signed_asset(&newer, asset) {
            Ok(signed) => signed,
            Err(refusal) => {
                let cause = refusal.to_string();
                tracing::warn!(
                    version = %version,
                    error = %cause,
                    "the newer release of Delta cannot be verified: not downloading it"
                );
                let unavailable = UpdateDownload::Unavailable { version, cause };
                *current = Some(unavailable.clone());
                return Ok(unavailable);
            }
        };

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
            asset = %signed.asset.name,
            url = %signed.asset.download_url,
            signature = %signed.signature_url,
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
            let done = match downloader.download(&signed, &dir, &report).await {
                Ok(path) => {
                    tracing::info!(
                        version = %version,
                        path = %path.display(),
                        "downloaded the newer release of Delta and verified its digest and signature"
                    );
                    UpdateDownload::Ready {
                        version,
                        path,
                        sha256,
                    }
                }
                Err(err @ AssetDownloadError::SignatureRejected(_)) => {
                    let cause = with_causes(&err);
                    tracing::warn!(
                        version = %version,
                        url = %signed.asset.download_url,
                        signature = %signed.signature_url,
                        error = %cause,
                        "the newer release of Delta failed its signature check: removed it"
                    );
                    UpdateDownload::Unavailable { version, cause }
                }
                Err(err) => {
                    let cause = with_causes(&err);
                    tracing::warn!(
                        version = %version,
                        url = %signed.asset.download_url,
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
        asset, linux_update, newer_release, settled, signature_of, signature_rejected, update_on,
        v060, GatedDownloader, GatedInstaller, LINUX_DEB, MAC_DMG,
    };
    use super::*;
    use crate::ports::AssetDownloadError;
    use crate::release_update::{Platform, UpdateOffer};

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
                sha256: "0".repeat(64),
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

    #[tokio::test]
    async fn the_download_is_handed_the_releases_signature_and_version() {
        let downloader = GatedDownloader::new();
        let update = linux_update(&downloader);
        update.start(Some(v060())).unwrap();
        downloader.finish(Ok(()));
        settled(&update).await;

        let signed = downloader.signed();
        assert_eq!(signed.len(), 1);
        assert_eq!(signed[0].asset, asset(LINUX_DEB));
        assert_eq!(
            signed[0].signature_url,
            signature_of(LINUX_DEB).download_url
        );
        assert_eq!(signed[0].version, semver::Version::new(0, 6, 0));
    }

    /// A release published without signatures is never downloaded: there is
    /// no fallback to its digest alone.
    #[tokio::test]
    async fn a_release_without_a_signature_is_unavailable_with_nothing_fetched() {
        for platform in [
            Platform::new("linux", "x86_64"),
            Platform::new("macos", "aarch64"),
        ] {
            let downloader = GatedDownloader::new();
            let installer = GatedInstaller::new();
            let update = update_on(platform.clone(), &downloader, &installer);
            let unsigned = newer_release("0.6.0", vec![asset(LINUX_DEB), asset(MAC_DMG)]);

            let answered = update.start(Some(unsigned.clone())).unwrap();
            let UpdateDownload::Unavailable { version, cause } = &answered else {
                panic!("{platform}: {answered:?}");
            };
            assert_eq!(version, "v0.6.0");
            assert!(cause.contains("carries no signature"), "{cause}");
            // Asked again, the same answer, still with nothing fetched.
            assert_eq!(update.start(Some(unsigned.clone())).unwrap(), answered);
            tokio::task::yield_now().await;
            assert_eq!(downloader.calls(), 0);
            assert_eq!(update.download_of(Some(&unsigned)), Some(answered));
            assert!(matches!(
                update.install(Some(unsigned)),
                Err(UpdateRefusal::NotReady)
            ));
            assert!(installer.asked().is_empty());
        }
    }

    /// The Linux and the macOS install alike: a file whose signature does
    /// not verify never becomes ready, so it is never handed to the
    /// installer (and on macOS never mounted or copied).
    #[tokio::test]
    async fn a_file_failing_its_signature_check_is_unavailable_and_never_installed() {
        for platform in [
            Platform::new("linux", "x86_64"),
            Platform::new("macos", "aarch64"),
        ] {
            let downloader = GatedDownloader::new();
            let installer = GatedInstaller::new();
            let update = update_on(platform.clone(), &downloader, &installer);
            assert!(update.installs());

            update.start(Some(v060())).unwrap();
            downloader.finish(Err(signature_rejected()));
            let done = settled(&update).await;
            assert_eq!(
                done,
                UpdateDownload::Unavailable {
                    version: "v0.6.0".into(),
                    cause: "the release's signature does not verify the downloaded file: \
                            the signature was made with another key than Delta's release key"
                        .into(),
                },
                "{platform}"
            );
            assert!(matches!(
                update.install(Some(v060())),
                Err(UpdateRefusal::NotReady)
            ));
            // Not downloaded again: the same release fails the same way.
            assert_eq!(update.start(Some(v060())).unwrap(), done);
            tokio::task::yield_now().await;
            assert_eq!(downloader.calls(), 1);
            assert!(installer.asked().is_empty());
            // A newer release than that is downloaded afresh.
            let v070 = newer_release(
                "0.7.0",
                vec![
                    asset("delta-desktop_0.7.0_amd64.deb"),
                    signature_of("delta-desktop_0.7.0_amd64.deb"),
                    asset("Delta_0.7.0_aarch64.dmg"),
                    signature_of("Delta_0.7.0_aarch64.dmg"),
                ],
            );
            assert!(matches!(
                update.start(Some(v070)).unwrap(),
                UpdateDownload::Downloading { .. }
            ));
            downloader.finish(Ok(()));
            settled(&update).await;
        }
    }
}
