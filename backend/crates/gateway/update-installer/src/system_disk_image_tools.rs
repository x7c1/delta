use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{DiskImageTools, ToolError};

/// `hdiutil`, by absolute path, so nothing on the user's `PATH` stands in
/// for it.
pub const HDIUTIL_PATH: &str = "/usr/bin/hdiutil";

/// `ditto`, by absolute path: the macOS copy that keeps a bundle's symlinks,
/// permissions and extended attributes, and so its code signature.
pub const DITTO_PATH: &str = "/usr/bin/ditto";

/// The [`DiskImageTools`] of macOS: [`HDIUTIL_PATH`] and [`DITTO_PATH`].
#[derive(Debug, Clone, Default)]
pub struct SystemDiskImageTools;

impl DiskImageTools for SystemDiskImageTools {
    fn attach(&self, image: &Path, mountpoint: &Path) -> Result<(), ToolError> {
        let mut command = Command::new(HDIUTIL_PATH);
        command
            .args([
                "attach",
                "-nobrowse",
                "-readonly",
                "-noautoopen",
                "-mountpoint",
            ])
            .arg(mountpoint)
            .arg(image);
        run(command, HDIUTIL_PATH)
    }

    fn detach(&self, mountpoint: &Path) -> Result<(), ToolError> {
        let mut command = Command::new(HDIUTIL_PATH);
        command.arg("detach").arg(mountpoint);
        run(command, HDIUTIL_PATH)
    }

    fn copy_bundle(&self, from: &Path, to: &Path) -> Result<(), ToolError> {
        let mut command = Command::new(DITTO_PATH);
        command.arg(from).arg(to);
        run(command, DITTO_PATH)
    }
}

/// Run `command` (`program`) to its end, reading nothing.
fn run(mut command: Command, program: &str) -> Result<(), ToolError> {
    let program = PathBuf::from(program);
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| match source.kind() {
            ErrorKind::NotFound => ToolError::Missing {
                program: program.clone(),
            },
            _ => ToolError::Unrunnable {
                program: program.clone(),
                source,
            },
        })?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    tracing::debug!(
        program = %program.display(),
        status = %output.status,
        stdout = %String::from_utf8_lossy(&output.stdout),
        stderr = %stderr,
        "a disk image tool ended"
    );
    if output.status.success() {
        return Ok(());
    }
    let said = stderr
        .lines()
        .map(str::trim)
        .rev()
        .find(|line| !line.is_empty())
        .map_or_else(
            || format!("it exited with {}", output.status),
            str::to_owned,
        );
    Err(ToolError::Failed { program, said })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_is_missing_and_a_failure_says_why() {
        assert!(matches!(
            run(Command::new("/nonexistent/hdiutil"), "/nonexistent/hdiutil"),
            Err(ToolError::Missing { program }) if program == Path::new("/nonexistent/hdiutil")
        ));

        let mut failing = Command::new("/bin/sh");
        failing
            .arg("-c")
            .arg("echo 'hdiutil: attach failed - image not recognized' >&2; exit 1");
        match run(failing, "/bin/sh") {
            Err(ToolError::Failed { said, .. }) => {
                assert_eq!(said, "hdiutil: attach failed - image not recognized")
            }
            other => panic!("{other:?}"),
        }

        let mut silent = Command::new("/bin/sh");
        silent.arg("-c").arg("exit 3");
        match run(silent, "/bin/sh") {
            Err(ToolError::Failed { said, .. }) => assert!(said.contains('3'), "{said}"),
            other => panic!("{other:?}"),
        }

        assert!(run(Command::new("/bin/true"), "/bin/true").is_ok());
    }
}
