//! The Linux [`UpdateInstaller`]: Delta's update helper, run as root through
//! `pkexec`.
//!
//! Delta's server never holds root. To install a downloaded update it runs
//!
//! ```text
//! pkexec /usr/lib/delta-desktop/delta-update-helper install --version <tag> --file <path>
//! ```
//!
//! where the helper is the only program the polkit action
//! `io.github.x7c1.delta.update` allows, at the path the `.deb` installs it
//! to ([`UPDATE_HELPER_PATH`]), and polkit asks for an administrator's
//! password every time. The helper checks the file again itself before
//! installing it. This crate only starts it and reads how it ended:
//!
//! | `pkexec` exit status | Meaning | [`InstallError`] |
//! |---|---|---|
//! | `0` | installed | — |
//! | `30` | the installed app is already the requested version or newer, so the helper installed nothing | — (installed) |
//! | `126` | the user dismissed the password dialog | [`InstallError::Dismissed`] |
//! | `127` | not authorized, or no polkit authentication agent | [`InstallError::Unavailable`] |
//! | `10`–`19` | the helper rejected the file itself (not a regular file, the digest, the package name or version); its last stderr line says why | [`InstallError::Rejected`] |
//! | any other | the helper could not check or install the file (GitHub out of reach, `dpkg` or `apt-get` failing); its last stderr line says why | [`InstallError::Failed`] |
//!
//! A missing `pkexec` or helper is [`InstallError::Unavailable`] too, found
//! before anything is run; a `pkexec` that is there but cannot be started is
//! [`InstallError::Unrunnable`].
//!
//! [`UpdateInstaller`]: delta_usecase::UpdateInstaller
//! [`InstallError`]: delta_usecase::InstallError
//! [`InstallError::Dismissed`]: delta_usecase::InstallError::Dismissed
//! [`InstallError::Unavailable`]: delta_usecase::InstallError::Unavailable
//! [`InstallError::Unrunnable`]: delta_usecase::InstallError::Unrunnable
//! [`InstallError::Rejected`]: delta_usecase::InstallError::Rejected
//! [`InstallError::Failed`]: delta_usecase::InstallError::Failed

mod pkexec_installer;
pub use pkexec_installer::PkexecInstaller;

/// Where the `.deb` installs Delta's update helper, root-owned: the one path
/// the polkit action allows (`org.freedesktop.policykit.exec.path`).
pub const UPDATE_HELPER_PATH: &str = "/usr/lib/delta-desktop/delta-update-helper";

/// `pkexec`, by absolute path, so nothing on the user's `PATH` stands in for
/// it.
pub const PKEXEC_PATH: &str = "/usr/bin/pkexec";
