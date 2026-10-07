//! The state of installing a downloaded update.

use delta_usecase::UpdateInstall;
use serde::Serialize;
use ts_rs::TS;

use super::WireManualInstall;

/// The state of installing a downloaded update, internally tagged by
/// `state`.
///
/// - `installing`: the installer runs, including while the system asks for
///   an administrator's password.
/// - `installed`: installed over the running app, or that version or a newer
///   one was installed already (from a terminal, say);
///   `POST /api/latest-release/restart` restarts into it.
/// - `rejected`: the installer rejected the file itself (not matching the
///   release's digest, or not Delta at the version asked for); `error` says
///   why. The file was removed and `download` is cleared, so the next step is
///   downloading the release again (`POST /api/latest-release/download`),
///   which clears this state. No `manual`: a file that failed the check is
///   never to be installed.
/// - `failed`: the installer could not check or install the file for another
///   reason (on Linux, GitHub out of reach or `apt-get` failing; on macOS,
///   `hdiutil` or `ditto` failing, or the new app bundle not renamed into
///   place); `error` says why.
///   A new request tries again.
/// - `unavailable`: Delta cannot install updates itself on this machine (on
///   Linux, no polkit agent, not authorized, or the programs it needs are
///   missing; on macOS, the app does not run from a `Delta.app` it may
///   replace); `error` says why. A new request tries again.
///
/// `failed` and `unavailable` carry `manual`, how the user installs the
/// verified file by hand ([`WireManualInstall`]). `version` is the release
/// the install is of (`v0.6.0`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(rename = "UpdateInstall")]
pub enum WireUpdateInstall {
    Installing {
        version: String,
    },
    Installed {
        version: String,
    },
    Rejected {
        version: String,
        error: String,
    },
    Failed {
        version: String,
        error: String,
        manual: WireManualInstall,
    },
    Unavailable {
        version: String,
        error: String,
        manual: WireManualInstall,
    },
}

impl From<UpdateInstall> for WireUpdateInstall {
    fn from(install: UpdateInstall) -> Self {
        match install {
            UpdateInstall::Installing { version } => Self::Installing { version },
            UpdateInstall::Installed { version, .. } => Self::Installed { version },
            UpdateInstall::Rejected { version, cause } => Self::Rejected {
                version,
                error: cause,
            },
            UpdateInstall::Failed {
                version,
                cause,
                manual,
            } => Self::Failed {
                version,
                error: cause,
                manual: manual.into(),
            },
            UpdateInstall::Unavailable {
                version,
                cause,
                manual,
            } => Self::Unavailable {
                version,
                error: cause,
                manual: manual.into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_install_state_serializes_in_snake_case() {
        assert_eq!(
            serde_json::to_value(WireUpdateInstall::Installed {
                version: "v0.6.0".to_owned()
            })
            .unwrap(),
            serde_json::json!({ "state": "installed", "version": "v0.6.0" })
        );
        assert_eq!(
            serde_json::to_value(WireUpdateInstall::Rejected {
                version: "v0.6.0".to_owned(),
                error: "digest mismatch".to_owned(),
            })
            .unwrap(),
            serde_json::json!({
                "state": "rejected",
                "version": "v0.6.0",
                "error": "digest mismatch",
            })
        );
        assert_eq!(
            serde_json::to_value(WireUpdateInstall::Unavailable {
                version: "v0.6.0".to_owned(),
                error: "no agent".to_owned(),
                manual: WireManualInstall::Command {
                    command: "sudo apt install /u/d.deb".to_owned(),
                },
            })
            .unwrap(),
            serde_json::json!({
                "state": "unavailable",
                "version": "v0.6.0",
                "error": "no agent",
                "manual": { "kind": "command", "command": "sudo apt install /u/d.deb" },
            })
        );
    }
}
