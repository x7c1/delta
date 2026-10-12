//! The operating system and CPU architecture a build runs on.

use std::fmt;
use std::path::Path;

use semver::Version;

use super::ManualInstall;

/// An operating system and CPU architecture, named like
/// [`std::env::consts::OS`] and [`std::env::consts::ARCH`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    os: String,
    arch: String,
}

impl Platform {
    /// The platform `os`/`arch` names.
    pub fn new(os: impl Into<String>, arch: impl Into<String>) -> Self {
        Self {
            os: os.into(),
            arch: arch.into(),
        }
    }

    /// The platform this build runs on.
    pub fn current() -> Self {
        Self::new(std::env::consts::OS, std::env::consts::ARCH)
    }

    /// The name of this platform's asset of `version`, by the release
    /// workflow's naming, or `None` for a platform no release carries.
    pub(super) fn asset_name(&self, version: &Version) -> Option<String> {
        match (self.os.as_str(), self.arch.as_str()) {
            ("linux", "x86_64") => Some(format!("delta-desktop_{version}_amd64.deb")),
            ("macos", "aarch64") => Some(format!("Delta_{version}_aarch64.dmg")),
            _ => None,
        }
    }

    /// Whether the app installs a downloaded update itself here: on Linux,
    /// where the update is a `.deb` a root helper installs, and on macOS,
    /// where the `Delta.app` in the update's disk image replaces the running
    /// one. Elsewhere the download is as far as the app goes.
    pub(super) fn installs_in_app(&self) -> bool {
        matches!(self.os.as_str(), "linux" | "macos")
    }

    /// How the user installs the verified download at `path` by hand here:
    /// open the disk image on macOS, `sudo apt install` it elsewhere (Linux,
    /// the only other platform that installs in the app).
    pub(super) fn manual_install(&self, path: &Path) -> ManualInstall {
        match self.os.as_str() {
            "macos" => ManualInstall::DiskImage(path.to_path_buf()),
            _ => ManualInstall::apt_install(path),
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.os, self.arch)
    }
}
