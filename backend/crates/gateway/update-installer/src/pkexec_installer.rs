use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use tokio::process::Command;

use delta_usecase::{InstallError, InstalledApp, UpdateInstaller};

use crate::{INSTALLED_APP_PATH, PKEXEC_PATH, UPDATE_HELPER_PATH};

/// `pkexec`'s exit status when the user dismissed the authentication dialog.
const PKEXEC_DISMISSED: i32 = 126;

/// `pkexec`'s exit status when the caller is not authorized, no
/// authentication agent could ask, or `pkexec` itself failed.
const PKEXEC_NOT_AUTHORIZED: i32 = 127;

/// The update helper's exit statuses that reject the file itself: not a
/// regular file, not matching the release's digest, or not `delta-desktop` at
/// the version asked for. The helper holds the same range
/// (`FILE_REJECTED` in its `refusal` module) and tests that each of its
/// refusals falls in it exactly when it rejects the file.
const HELPER_REJECTED_FILE: std::ops::RangeInclusive<i32> = 10..=19;

/// The update helper's exit status when the installed app is already the
/// requested version or newer (installed from a terminal, say), so it
/// installed nothing: the update is installed all the same. The helper holds
/// the same status (`ALREADY_INSTALLED` in its `refusal` module).
const HELPER_ALREADY_INSTALLED: i32 = 30;

/// The [`UpdateInstaller`] that runs Delta's update helper through `pkexec`.
#[derive(Debug, Clone)]
pub struct PkexecInstaller {
    pkexec: PathBuf,
    helper: PathBuf,
}

impl PkexecInstaller {
    /// The installer of an installed Delta: [`PKEXEC_PATH`] running the
    /// helper at [`UPDATE_HELPER_PATH`].
    pub fn new() -> Self {
        Self::with_paths(PKEXEC_PATH, UPDATE_HELPER_PATH)
    }

    /// An installer running `helper` through `pkexec`, wherever they are.
    pub fn with_paths(pkexec: impl Into<PathBuf>, helper: impl Into<PathBuf>) -> Self {
        Self {
            pkexec: pkexec.into(),
            helper: helper.into(),
        }
    }
}

impl Default for PkexecInstaller {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UpdateInstaller for PkexecInstaller {
    /// The helper checks the file against the release's digest itself, as
    /// root, so `sha256` is not handed to it.
    async fn install(
        &self,
        version: &str,
        file: &Path,
        _sha256: &str,
    ) -> Result<InstalledApp, InstallError> {
        if !self.helper.is_file() {
            return Err(InstallError::Unavailable(format!(
                "the update helper {} is not installed",
                self.helper.display()
            )));
        }
        let output = Command::new(&self.pkexec)
            .arg(&self.helper)
            .args(["install", "--version", version, "--file"])
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|source| match source.kind() {
                ErrorKind::NotFound => {
                    InstallError::Unavailable(format!("{} is not installed", self.pkexec.display()))
                }
                _ => InstallError::Unrunnable {
                    program: self.pkexec.clone(),
                    source,
                },
            })?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::debug!(
            status = %output.status,
            stdout = %String::from_utf8_lossy(&output.stdout),
            stderr = %stderr,
            "the update helper ended"
        );
        outcome(output.status.code(), &stderr)
            .map(|()| InstalledApp::Executable(PathBuf::from(INSTALLED_APP_PATH)))
    }
}

/// What `pkexec` ending with `code` (`None` when a signal ended it) and
/// printing `stderr` means for the install.
fn outcome(code: Option<i32>, stderr: &str) -> Result<(), InstallError> {
    let said = last_line(stderr);
    match code {
        Some(0) => Ok(()),
        Some(HELPER_ALREADY_INSTALLED) => {
            tracing::info!(
                reason = said.unwrap_or_default(),
                "the update is installed already: the helper installed nothing"
            );
            Ok(())
        }
        Some(PKEXEC_DISMISSED) => Err(InstallError::Dismissed),
        Some(PKEXEC_NOT_AUTHORIZED) => Err(InstallError::Unavailable(match said {
            Some(said) => format!(
                "not authorized, or no polkit authentication agent is running (pkexec: {said})"
            ),
            None => "not authorized, or no polkit authentication agent is running".to_owned(),
        })),
        Some(code) => {
            let reason = said.map_or_else(
                || format!("the update helper exited with status {code}"),
                str::to_owned,
            );
            Err(if HELPER_REJECTED_FILE.contains(&code) {
                InstallError::Rejected(reason)
            } else {
                InstallError::Failed(reason)
            })
        }
        None => Err(InstallError::Failed(
            "the update helper was ended by a signal".to_owned(),
        )),
    }
}

/// The last non-empty line of `text`, trimmed.
fn last_line(text: &str) -> Option<&str> {
    text.lines()
        .map(str::trim)
        .rev()
        .find(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA256: &str = "ff";

    #[test]
    fn each_exit_status_maps_to_its_outcome() {
        assert!(outcome(Some(0), "").is_ok());
        // The installed app is already that version or newer: installed.
        assert!(outcome(
            Some(30),
            "the update file's version 0.6.0 is not newer than the installed 0.6.0\n"
        )
        .is_ok());
        assert!(matches!(
            outcome(
                Some(126),
                "Error executing command as another user: Request dismissed"
            ),
            Err(InstallError::Dismissed)
        ));
        match outcome(
            Some(127),
            "Error executing command as another user: No authentication agent found.\n",
        ) {
            Err(InstallError::Unavailable(cause)) => {
                assert!(cause.contains("No authentication agent found."), "{cause}")
            }
            other => panic!("{other:?}"),
        }
        match outcome(
            Some(14),
            "the update file's sha256 is 00, not release v0.6.0's ff\n\n",
        ) {
            Err(InstallError::Rejected(reason)) => assert_eq!(
                reason,
                "the update file's sha256 is 00, not release v0.6.0's ff"
            ),
            other => panic!("{other:?}"),
        }
        // Every status from 10 to 19 rejects the file; the ones around do not.
        for code in 10..=19 {
            assert!(
                matches!(outcome(Some(code), ""), Err(InstallError::Rejected(_))),
                "{code}"
            );
        }
        for code in [2, 3, 9, 25, 26, 29, 31] {
            assert!(
                matches!(outcome(Some(code), ""), Err(InstallError::Failed(_))),
                "{code}"
            );
        }
        match outcome(Some(20), "") {
            Err(InstallError::Failed(reason)) => {
                assert_eq!(reason, "the update helper exited with status 20")
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(outcome(None, ""), Err(InstallError::Failed(_))));
    }

    #[tokio::test]
    async fn a_missing_helper_or_pkexec_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("helper");
        let file = dir.path().join("d.deb");
        let missing_helper = PkexecInstaller::with_paths("/bin/sh", &helper);
        assert!(matches!(
            missing_helper.install("v0.6.0", &file, SHA256).await,
            Err(InstallError::Unavailable(cause)) if cause.contains("is not installed")
        ));

        std::fs::write(&helper, "exit 0\n").unwrap();
        let missing_pkexec = PkexecInstaller::with_paths(dir.path().join("pkexec"), &helper);
        assert!(matches!(
            missing_pkexec.install("v0.6.0", &file, SHA256).await,
            Err(InstallError::Unavailable(cause)) if cause.contains("is not installed")
        ));

        // A `pkexec` that is there but not executable cannot be started.
        let unrunnable = dir.path().join("pkexec");
        std::fs::write(&unrunnable, "").unwrap();
        assert!(matches!(
            PkexecInstaller::with_paths(&unrunnable, &helper)
                .install("v0.6.0", &file, SHA256)
                .await,
            Err(InstallError::Unrunnable { program, source })
                if program == unrunnable && source.kind() == ErrorKind::PermissionDenied
        ));
    }

    /// A helper that found the update installed already ends the install as
    /// installed.
    #[tokio::test]
    async fn a_helper_finding_the_update_installed_already_installs_it() {
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("helper");
        std::fs::write(
            &helper,
            "echo \"the update file's version 0.6.0 is not newer than the installed 0.6.0\" >&2\nexit 30\n",
        )
        .unwrap();
        let file = dir.path().join("delta-desktop_0.6.0_amd64.deb");
        let result = PkexecInstaller::with_paths("/bin/sh", &helper)
            .install("v0.6.0", &file, SHA256)
            .await;
        assert_eq!(
            result.unwrap(),
            InstalledApp::Executable(PathBuf::from("/usr/bin/delta-desktop"))
        );
    }

    /// `/bin/sh` stands in for `pkexec`: it runs the helper script with the
    /// arguments `pkexec` would hand the helper.
    #[tokio::test]
    async fn the_helper_is_run_with_the_version_and_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let args = dir.path().join("args");
        let helper = dir.path().join("helper");
        std::fs::write(
            &helper,
            format!(
                "printf '%s\\n' \"$@\" > '{}'\necho 'apt-get could not install the update: E: broken' >&2\nexit 20\n",
                args.display()
            ),
        )
        .unwrap();
        let file = dir.path().join("delta-desktop_0.6.0_amd64.deb");

        let result = PkexecInstaller::with_paths("/bin/sh", &helper)
            .install("v0.6.0", &file, SHA256)
            .await;
        assert!(
            matches!(&result, Err(InstallError::Failed(reason)) if reason == "apt-get could not install the update: E: broken"),
            "{result:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&args).unwrap(),
            format!("install\n--version\nv0.6.0\n--file\n{}\n", file.display())
        );
    }
}
