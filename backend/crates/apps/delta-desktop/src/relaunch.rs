//! Starting the app again after an update was installed over it.
//!
//! The running process cannot start its successor directly: the
//! single-instance plugin would see this copy still running and make the new
//! one exit at once. So the desktop shell exits after starting a detached
//! `/bin/sh` that waits for this process to exit and then starts the
//! installed app: on Linux by running its binary, on macOS by `open`ing its
//! bundle, as Finder would.
//!
//! What is started is what the installer named when it installed the update
//! ([`InstalledApp`]), never where this process says it was started from:
//! once dpkg has replaced the binary, this process's own executable link
//! names a deleted file, and once a macOS update has swapped the bundle, this
//! process runs from the backup of the old one.

use std::ffi::OsStr;
use std::io;
use std::process::{Command, Stdio};

use delta_bootstrap::InstalledApp;

/// `open`, by absolute path: how a macOS app bundle is started.
const OPEN: &str = "/usr/bin/open";

/// What the detached shell runs: wait while process `$1` exists, then
/// replace itself with the command in the rest of its arguments.
const WAIT_THEN_RUN: &str =
    r#"while kill -0 "$1" 2>/dev/null; do sleep 0.1; done; shift; exec "$@""#;

/// The command that waits for process `pid` to exit and then starts `app`, in
/// a process group of its own so that nothing aimed at this process's group
/// reaches it. It reads nothing and keeps this process's stdout and stderr,
/// so the new app logs where this one did.
pub fn relaunch_command(pid: u32, app: &InstalledApp) -> Command {
    use std::os::unix::process::CommandExt;

    let start: Vec<&OsStr> = match app {
        InstalledApp::Executable(path) => vec![path.as_os_str()],
        InstalledApp::Bundle(bundle) => vec![OsStr::new(OPEN), bundle.as_os_str()],
    };
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(WAIT_THEN_RUN)
        .arg("delta-relaunch")
        .arg(pid.to_string())
        .args(start)
        .stdin(Stdio::null())
        .process_group(0);
    command
}

/// Start `app` again once this process has exited. The caller exits next.
pub fn relaunch_after_exit(app: &InstalledApp) -> io::Result<()> {
    relaunch_command(std::process::id(), app).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn args(command: &Command) -> Vec<&OsStr> {
        command.get_args().collect()
    }

    #[test]
    fn on_linux_the_relaunch_waits_for_the_pid_then_runs_the_installed_binary() {
        let command = relaunch_command(
            4242,
            &InstalledApp::Executable(PathBuf::from("/usr/bin/delta-desktop")),
        );
        assert_eq!(command.get_program(), "/bin/sh");
        assert_eq!(
            args(&command),
            [
                OsStr::new("-c"),
                OsStr::new(WAIT_THEN_RUN),
                OsStr::new("delta-relaunch"),
                OsStr::new("4242"),
                OsStr::new("/usr/bin/delta-desktop"),
            ]
        );
    }

    #[test]
    fn on_macos_the_relaunch_waits_for_the_pid_then_opens_the_bundle() {
        let command = relaunch_command(
            4242,
            &InstalledApp::Bundle(PathBuf::from("/Applications/Delta.app")),
        );
        assert_eq!(command.get_program(), "/bin/sh");
        assert_eq!(
            args(&command),
            [
                OsStr::new("-c"),
                OsStr::new(WAIT_THEN_RUN),
                OsStr::new("delta-relaunch"),
                OsStr::new("4242"),
                OsStr::new("/usr/bin/open"),
                OsStr::new("/Applications/Delta.app"),
            ]
        );
    }

    /// Runs the script for real: it waits while the process lives, and runs
    /// the app only once it has exited, with the arguments it was given.
    #[test]
    fn the_relaunch_runs_the_app_only_after_the_process_exits() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let app = dir.path().join("app");
        std::fs::write(
            &app,
            format!("#!/bin/sh\necho \"$#\" > '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&app, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();

        let mut waited_on = Command::new("/bin/sleep").arg("0.5").spawn().unwrap();
        let mut relaunch = relaunch_command(waited_on.id(), &InstalledApp::Executable(app))
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!marker.exists(), "the app ran while the process was alive");
        waited_on.wait().unwrap();
        assert!(relaunch.wait().unwrap().success());
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "0\n",
            "the app did not run, or ran with arguments, after the process exited"
        );
    }
}
