//! How the user installs a downloaded update by hand.

use delta_usecase::ManualInstall;
use serde::Serialize;
use ts_rs::TS;

/// How the user installs the verified download by hand when Delta cannot,
/// internally tagged by `kind`, one per platform:
///
/// - `command` (Linux): commands to run in a terminal outside Delta (not a
///   Delta session's pane, which agents can type into). `check` checks the
///   file first, one command per line: `sha256sum <path>`, to compare with
///   the digest on the release page, and `minisign -Vm <path> -P <release
///   public key>`, against the signature Delta kept beside it. `install`
///   installs it: `sudo -k apt install <absolute path of the .deb>`, so
///   `sudo` asks for the password and keeps no cached authentication. Each
///   path is quoted for a POSIX shell when it needs to be.
/// - `disk_image` (macOS): open the disk image at `path`, an absolute path,
///   quit Delta (Finder does not replace an app that is open), drag Delta to
///   Applications, and start Delta again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(rename = "ManualInstall")]
pub enum WireManualInstall {
    Command { check: Vec<String>, install: String },
    DiskImage { path: String },
}

impl From<ManualInstall> for WireManualInstall {
    fn from(manual: ManualInstall) -> Self {
        match manual {
            ManualInstall::Command { check, install } => Self::Command { check, install },
            ManualInstall::DiskImage(path) => Self::DiskImage {
                path: path.to_string_lossy().into_owned(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn each_kind_serializes_tagged_in_snake_case() {
        assert_eq!(
            serde_json::to_value(WireManualInstall::from(ManualInstall::Command {
                check: vec!["sha256sum /u/d.deb".to_owned()],
                install: "sudo -k apt install /u/d.deb".to_owned(),
            }))
            .unwrap(),
            serde_json::json!({
                "kind": "command",
                "check": ["sha256sum /u/d.deb"],
                "install": "sudo -k apt install /u/d.deb",
            })
        );
        assert_eq!(
            serde_json::to_value(WireManualInstall::from(ManualInstall::DiskImage(
                PathBuf::from("/Users/u/Library/Application Support/io.github.x7c1.delta/updates/Delta_0.6.0_aarch64.dmg")
            )))
            .unwrap(),
            serde_json::json!({
                "kind": "disk_image",
                "path": "/Users/u/Library/Application Support/io.github.x7c1.delta/updates/Delta_0.6.0_aarch64.dmg",
            })
        );
    }
}
