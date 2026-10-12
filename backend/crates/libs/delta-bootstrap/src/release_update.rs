//! Wiring the in-app update to the configured build.

use std::sync::Arc;

use delta_usecase::{AssetDownloader, NotOffered, Platform, ReleaseUpdate, UpdateInstaller};
use release_feed::{remove_stale_updates, GithubAssetDownloader};
use update_installer::{BundleInstaller, PkexecInstaller};

use crate::Config;

/// The in-app update `config` allows: offered only to a desktop app built by
/// the release workflow ([`NotOffered::of_build`]), downloading this
/// platform's asset into [`DataLayout::updates`](crate::DataLayout::updates)
/// and installing it through [`installer`].
///
/// First removes the downloads in that directory that `current_version`, the
/// running app's version, is not older than ([`remove_stale_updates`]), so an
/// update the app has restarted into does not linger. This runs for every
/// build, offered updates or not, since the CLI server may share the desktop
/// app's data directory. A `current_version` that is not a version removes
/// nothing. On macOS it also removes the bundle an earlier update replaced,
/// kept next to the running one until now
/// ([`BundleInstaller::remove_update_leftovers`]).
///
/// The HTTPS client is built here, never on a request, and only when updates
/// are offered; when it fails, updates are not offered, with a `warn`, since a
/// missing update must never stop the server.
pub fn release_update(config: &Config, current_version: &str) -> ReleaseUpdate {
    let updates = config.data_layout().updates();
    match semver::Version::parse(current_version) {
        Ok(running) => {
            remove_stale_updates(&updates, &running);
        }
        Err(err) => tracing::warn!(
            version = current_version,
            error = %err,
            "not clearing old downloaded updates: the running version is not a version"
        ),
    }
    let installer = installer();
    if let Some(reason) = NotOffered::of_build(config.launcher, config.build_origin) {
        return ReleaseUpdate::not_offered(reason);
    }
    match GithubAssetDownloader::new() {
        Ok(downloader) => ReleaseUpdate::offered(
            Platform::current(),
            updates,
            Arc::new(downloader) as Arc<dyn AssetDownloader>,
            installer,
        ),
        Err(err) => {
            tracing::warn!(error = %err, "updates are not offered");
            ReleaseUpdate::not_offered(NotOffered::NoDownloader)
        }
    }
}

/// This platform's installer: on macOS the [`BundleInstaller`] of the
/// running app, found now, before any update replaced it, after removing
/// what an earlier update left next to it; elsewhere Delta's update helper
/// under `pkexec` (used on Linux only, the one other platform that installs
/// in the app).
fn installer() -> Arc<dyn UpdateInstaller> {
    if cfg!(target_os = "macos") {
        let installer = BundleInstaller::for_running_app();
        installer.remove_update_leftovers();
        Arc::new(installer)
    } else {
        Arc::new(PkexecInstaller::new())
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
            let err = release_update(&config, "0.5.0").start(None).unwrap_err();
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

    #[test]
    fn startup_removes_the_updates_the_running_version_is_not_older_than() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(&dir);
        let updates = config.data_layout().updates();
        std::fs::create_dir_all(&updates).unwrap();
        for name in [
            "delta-desktop_0.4.9_amd64.deb",
            "delta-desktop_0.5.0_amd64.deb",
            "delta-desktop_0.6.0_amd64.deb",
        ] {
            std::fs::write(updates.join(name), name).unwrap();
        }
        release_update(&config, "0.5.0");
        let left: Vec<_> = std::fs::read_dir(&updates)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["delta-desktop_0.6.0_amd64.deb"]);
    }
}
