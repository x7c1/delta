//! Delta's update helper: installs a downloaded update of the desktop app on
//! Linux, as root.
//!
//! The only program that runs as root for an update. Delta's server starts it
//! through `pkexec`, under a polkit action of its own that allows exactly this
//! program at its installed path and asks for an administrator's password
//! every time (`linux/io.github.x7c1.delta.update.policy` in the desktop
//! crate). It accepts one command line,
//!
//! ```text
//! delta-update-helper install --version v<version> --file <absolute path>
//! ```
//!
//! and trusts nothing in it beyond which version to install and which file
//! holds it. Before installing, it checks everything itself:
//!
//! 1. copies the file into a new root-owned directory (mode 0700), refusing a
//!    symlink or anything but a regular file, so the file cannot be swapped
//!    after it is checked ([`StagedCopy`]);
//! 2. asks GitHub for the release the tag names
//!    (`https://api.github.com/repos/x7c1/delta/releases/tags/<tag>`) and
//!    compares the copy's sha256 with the digest it states for the `.deb`
//!    ([`release_digest`]); a release without one is refused;
//! 3. checks with `dpkg-deb` and `dpkg-query` that the package is
//!    `delta-desktop`, at the requested version, and newer than the installed
//!    one ([`package_check`]); when the installed one is that version or
//!    newer already, it installs nothing and exits with
//!    [`refusal::ALREADY_INSTALLED`], which the server reads as installed;
//! 4. installs the copy with `apt-get install -y <path>`, so its dependencies
//!    resolve, then removes the directory.
//!
//! Every refusal and failure exits with a status of its own and prints one
//! line on stderr saying why ([`Refusal`]), which the server reports. A
//! status in [`refusal::FILE_REJECTED`] says the file itself is not to be
//! installed, so the server removes it and has the user download it again;
//! after any other, the user may still install it from a terminal. The
//! decisions live in functions the unit tests cover without root; this file
//! is only the thin shell that runs `dpkg-deb`, `dpkg-query` and `apt-get`.

mod install_request;
mod package_check;
mod refusal;
mod release_digest;
mod staged_copy;

use std::path::Path;
use std::process::{Command, ExitCode, Output};

use install_request::InstallRequest;
use refusal::Refusal;
use staged_copy::StagedCopy;

/// The programs the helper runs, by absolute path: `pkexec` hands it a
/// minimal environment, and root should not look anything up on a `PATH`.
const DPKG_DEB: &str = "/usr/bin/dpkg-deb";
const DPKG_QUERY: &str = "/usr/bin/dpkg-query";
const APT_GET: &str = "/usr/bin/apt-get";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(refusal) => {
            eprintln!("{refusal}");
            let code = refusal.exit_code();
            debug_assert_eq!(
                refusal::FILE_REJECTED.contains(&code),
                refusal.rejects_file()
            );
            ExitCode::from(code)
        }
    }
}

fn run() -> Result<(), Refusal> {
    let request = InstallRequest::parse(std::env::args_os().skip(1))?;
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    if unsafe { libc::geteuid() } != 0 {
        return Err(Refusal::NotRoot);
    }

    let staged = StagedCopy::copy(&request.file, &std::env::temp_dir())?;

    let asset = release_digest::deb_asset_name(
        &request.version,
        release_digest::deb_arch(std::env::consts::ARCH),
    );
    let expected = release_digest::fetch_digest(&request.tag, &asset)?;
    if staged.sha256() != expected {
        return Err(Refusal::DigestMismatch {
            tag: request.tag,
            expected,
            actual: staged.sha256().to_owned(),
        });
    }

    let fields = run_reading(
        DPKG_DEB,
        &[
            "--field".as_ref(),
            staged.path().as_os_str(),
            "Package".as_ref(),
            "Version".as_ref(),
        ],
        "the update file's package name and version",
    )?;
    let (name, version) = package_check::package_fields(&fields)?;
    let installed = run_reading(
        DPKG_QUERY,
        &[
            "--showformat=${Version}".as_ref(),
            "--show".as_ref(),
            package_check::PACKAGE_NAME.as_ref(),
        ],
        "the installed delta-desktop's version",
    )?;
    package_check::check_package(&name, &version, &request.version, &installed)?;

    install(&staged.path())
}

/// Run `program` with `args` and return its stdout, refusing with what it
/// printed on stderr when it fails.
fn run_reading(
    program: &'static str,
    args: &[&std::ffi::OsStr],
    what: &'static str,
) -> Result<String, Refusal> {
    let unreadable = |cause: String| Refusal::PackageUnreadable { what, cause };
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|source| Refusal::Unrunnable { program, source })?;
    if !output.status.success() {
        return Err(unreadable(format!(
            "{program} {}: {}",
            output.status,
            last_line(&output)
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| unreadable(format!("{program} printed something that is not UTF-8")))
}

/// Install the package at `path` with `apt-get`, so that new dependencies are
/// installed with it.
fn install(path: &Path) -> Result<(), Refusal> {
    let output = Command::new(APT_GET)
        .args(["install", "-y"])
        .arg(path)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .output()
        .map_err(|source| Refusal::Unrunnable {
            program: APT_GET,
            source,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Refusal::InstallFailed(format!(
            "{} ({})",
            last_line(&output),
            output.status
        )))
    }
}

/// The last non-empty line a command printed on stderr, or on stdout when
/// stderr is empty.
fn last_line(output: &Output) -> String {
    [&output.stderr, &output.stdout]
        .into_iter()
        .find_map(|stream| {
            String::from_utf8_lossy(stream)
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_owned())
        })
        .unwrap_or_else(|| "no output".to_owned())
}
