//! Why updating is not offered: decided by who launched the server and where
//! it was built.

use super::{BuildOrigin, Launcher, UpdateOffer};

/// Why updating is not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotOffered {
    /// The CLI launched the server.
    CliLauncher,
    /// A desktop app built locally.
    LocalBuild,
    /// A desktop release build whose HTTPS client could not be set up, so it
    /// cannot download anything.
    NoDownloader,
}

impl NotOffered {
    /// Why a server `launcher` launched from a build of `origin` may not
    /// update, or `None` when it may: only a desktop app built by the release
    /// workflow.
    pub fn of_build(launcher: Launcher, origin: BuildOrigin) -> Option<Self> {
        match (launcher, origin) {
            (Launcher::Cli, _) => Some(Self::CliLauncher),
            (Launcher::Desktop, BuildOrigin::Local) => Some(Self::LocalBuild),
            (Launcher::Desktop, BuildOrigin::Release) => None,
        }
    }

    /// What the browser may offer instead of Update.
    pub fn offer(self) -> UpdateOffer {
        match self {
            Self::LocalBuild => UpdateOffer::Rebuild,
            Self::CliLauncher | Self::NoDownloader => UpdateOffer::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_desktop_release_build_may_update() {
        use BuildOrigin::{Local, Release};
        use Launcher::{Cli, Desktop};
        assert_eq!(NotOffered::of_build(Desktop, Release), None);
        assert_eq!(
            NotOffered::of_build(Desktop, Local),
            Some(NotOffered::LocalBuild)
        );
        for origin in [Release, Local] {
            assert_eq!(
                NotOffered::of_build(Cli, origin),
                Some(NotOffered::CliLauncher)
            );
        }
    }

    #[test]
    fn a_local_desktop_build_is_offered_the_rebuild_hint() {
        assert_eq!(NotOffered::LocalBuild.offer(), UpdateOffer::Rebuild);
        assert_eq!(NotOffered::CliLauncher.offer(), UpdateOffer::None);
        assert_eq!(NotOffered::NoDownloader.offer(), UpdateOffer::None);
    }
}
