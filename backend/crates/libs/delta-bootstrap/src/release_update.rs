//! Wiring the update download to the configured build.

use std::sync::Arc;

use delta_usecase::{AssetDownloader, NotOffered, Platform, ReleaseUpdate};
use release_feed::GithubAssetDownloader;

use crate::Config;

/// The update download `config` allows: offered only to a desktop app built
/// by the release workflow ([`NotOffered::of_build`]), downloading this
/// platform's asset into [`DataLayout::updates`](crate::DataLayout::updates).
///
/// The HTTPS client is built here, never on a request, and only when updates
/// are offered; when it fails, updates are not offered, with a `warn`, since a
/// missing update must never stop the server.
pub fn release_update(config: &Config) -> ReleaseUpdate {
    if let Some(reason) = NotOffered::of_build(config.launcher, config.build_origin) {
        return ReleaseUpdate::not_offered(reason);
    }
    match GithubAssetDownloader::new() {
        Ok(downloader) => ReleaseUpdate::offered(
            Platform::current(),
            config.data_layout().updates(),
            Arc::new(downloader) as Arc<dyn AssetDownloader>,
        ),
        Err(err) => {
            tracing::warn!(error = %err, "updates are not offered");
            ReleaseUpdate::not_offered(NotOffered::NoDownloader)
        }
    }
}

#[cfg(test)]
mod tests {
    use delta_usecase::{BuildOrigin, Launcher, UpdateRefusal};

    use super::*;
    use crate::build::testing::test_config;

    #[test]
    fn the_update_follows_the_launcher_and_the_build_origin() {
        let dir = tempfile::tempdir().unwrap();
        for (launcher, build_origin) in [
            (Launcher::Cli, BuildOrigin::Release),
            (Launcher::Cli, BuildOrigin::Local),
            (Launcher::Desktop, BuildOrigin::Local),
            (Launcher::Desktop, BuildOrigin::Release),
        ] {
            let config = Config {
                launcher,
                build_origin,
                ..test_config(&dir)
            };
            // Without a newer release nothing is fetched either way, so the
            // refusal tells a build that may not update from one that may.
            let err = release_update(&config).start(None).unwrap_err();
            let refused_for_the_build = match (launcher, build_origin) {
                (Launcher::Cli, _) => matches!(err, UpdateRefusal::CliLauncher),
                (Launcher::Desktop, BuildOrigin::Local) => {
                    matches!(err, UpdateRefusal::LocalBuild)
                }
                (Launcher::Desktop, BuildOrigin::Release) => {
                    matches!(err, UpdateRefusal::NoNewerRelease)
                }
            };
            assert!(
                refused_for_the_build,
                "{launcher:?} {build_origin:?}: {err:?}"
            );
        }
    }
}
