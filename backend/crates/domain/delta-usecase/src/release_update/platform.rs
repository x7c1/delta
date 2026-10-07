//! The operating system and CPU architecture a build runs on.

use std::fmt;

use semver::Version;

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
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.os, self.arch)
    }
}
