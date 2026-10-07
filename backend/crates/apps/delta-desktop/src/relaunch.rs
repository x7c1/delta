//! Starting the app again after an update was installed over it.
//!
//! The running process cannot start its successor directly: the
//! single-instance plugin would see this copy still running and make the new
//! one exit at once. So the desktop shell exits after starting a detached
//! `/bin/sh` that waits for this process to exit and then runs the installed
//! app.
//!
//! The app is started from the path the package installs it to, not from
//! wherever this process was started: once dpkg has replaced the binary,
//! this process's own executable link names a deleted file.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// Where the `.deb` installs the app (Linux, the only platform the app
/// installs updates on so far).
#[cfg(target_os = "linux")]
const INSTALLED_APP: &str = "/usr/bin/delta-desktop";

/// What the detached shell runs: wait while process `$1` exists, then
/// replace itself with `$2`.
const WAIT_THEN_RUN: &str = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.1; done; exec "$2""#;

/// The installed app to start again after an update, or `None` on a
/// platform where the app installs no updates.
pub fn installed_app() -> Option<&'static Path> {
    #[cfg(target_os = "linux")]
    return Some(Path::new(INSTALLED_APP));
    #[cfg(not(target_os = "linux"))]
    return None;
}

/// The command that waits for process `pid` to exit and then runs `app`, in
/// a process group of its own so that nothing aimed at this process's group
/// reaches it. It reads nothing and keeps this process's stdout and stderr,
/// so the new app logs where this one did.
pub fn relaunch_command(pid: u32, app: &Path) -> Command {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(WAIT_THEN_RUN)
        .arg("delta-relaunch")
        .arg(pid.to_string())
        .arg(app)
        .stdin(Stdio::null())
        .process_group(0);
    command
}

/// Start `app` again once this process has exited. The caller exits next.
pub fn relaunch_after_exit(app: &Path) -> io::Result<()> {
    relaunch_command(std::process::id(), app).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn the_relaunch_waits_for_the_pid_then_runs_the_installed_app() {
        let command = relaunch_command(4242, Path::new("/usr/bin/delta-desktop"));
        assert_eq!(command.get_program(), "/bin/sh");
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(
            args,
            [
                OsStr::new("-c"),
                OsStr::new(WAIT_THEN_RUN),
                OsStr::new("delta-relaunch"),
                OsStr::new("4242"),
                OsStr::new("/usr/bin/delta-desktop"),
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_installed_app_is_the_packages_binary() {
        assert_eq!(installed_app(), Some(Path::new("/usr/bin/delta-desktop")));
    }

    /// Runs the script for real: it waits while the process lives, and runs
    /// the app only once it has exited.
    #[test]
    fn the_relaunch_runs_the_app_only_after_the_process_exits() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let app = dir.path().join("app");
        std::fs::write(&app, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        std::fs::set_permissions(&app, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();

        let mut waited_on = Command::new("/bin/sleep").arg("0.5").spawn().unwrap();
        let mut relaunch = relaunch_command(waited_on.id(), &app).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!marker.exists(), "the app ran while the process was alive");
        waited_on.wait().unwrap();
        assert!(relaunch.wait().unwrap().success());
        assert!(
            marker.exists(),
            "the app did not run after the process exited"
        );
    }
}
