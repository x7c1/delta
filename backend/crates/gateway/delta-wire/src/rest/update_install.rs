//! The state of installing a downloaded update.

use delta_usecase::UpdateInstall;
use serde::Serialize;
use ts_rs::TS;

/// The state of installing a downloaded update, internally tagged by
/// `state`.
///
/// - `installing`: the installer runs, including while the system asks for
///   an administrator's password.
/// - `installed`: installed over the running app, or that version or a newer
///   one was installed already (from a terminal, say);
///   `POST /api/latest-release/restart` restarts into it.
/// - `rejected`: the update helper rejected the file itself (not matching
///   the release's digest, or not `delta-desktop` at the version asked for);
///   `error` says why. The file was removed and `download` is cleared, so the
///   next step is downloading the release again
///   (`POST /api/latest-release/download`), which clears this state. No
///   `manual_command`: a file that failed the check is never to be
///   installed.
/// - `failed`: the helper could not check or install the file for another
///   reason (GitHub out of reach, `apt-get` failing); `error` says why. A new
///   request tries again.
/// - `unavailable`: Delta cannot install updates itself on this machine (no
///   polkit agent, not authorized, or the programs it needs are missing);
///   `error` says why. A new request tries again.
///
/// `failed` and `unavailable` carry `manual_command`, the command that
/// installs the verified file from the user's own terminal
/// (`sudo apt install <path>`). `version` is the release the install is of
/// (`v0.6.0`).
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
        manual_command: String,
    },
    Unavailable {
        version: String,
        error: String,
        manual_command: String,
    },
}

impl From<UpdateInstall> for WireUpdateInstall {
    fn from(install: UpdateInstall) -> Self {
        match install {
            UpdateInstall::Installing { version } => Self::Installing { version },
            UpdateInstall::Installed { version } => Self::Installed { version },
            UpdateInstall::Rejected { version, cause } => Self::Rejected {
                version,
                error: cause,
            },
            UpdateInstall::Failed {
                version,
                cause,
                manual_command,
            } => Self::Failed {
                version,
                error: cause,
                manual_command,
            },
            UpdateInstall::Unavailable {
                version,
                cause,
                manual_command,
            } => Self::Unavailable {
                version,
                error: cause,
                manual_command,
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
                manual_command: "sudo apt install /u/d.deb".to_owned(),
            })
            .unwrap(),
            serde_json::json!({
                "state": "unavailable",
                "version": "v0.6.0",
                "error": "no agent",
                "manual_command": "sudo apt install /u/d.deb",
            })
        );
    }
}
