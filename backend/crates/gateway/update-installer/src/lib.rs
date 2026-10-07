//! The [`UpdateInstaller`]s: on Linux, Delta's update helper run as root
//! through `pkexec` ([`PkexecInstaller`]); on macOS, replacing the running
//! `Delta.app` with the one in the downloaded disk image, as the user
//! ([`BundleInstaller`]).
//!
//! # Linux
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
//! [`InstallError::Unrunnable`]. The app to restart into is the one the
//! `.deb` installs, [`INSTALLED_APP_PATH`].
//!
//! # macOS
//!
//! Nothing runs as root and nothing asks for a password: the bundle is
//! replaced by the user running Delta, where that user may write. The
//! running bundle is found once, when the installer is made at startup,
//! from the server's executable (`<dir>/Delta.app/Contents/MacOS/<exe>` →
//! `<dir>/Delta.app`, wherever `<dir>` is). Steps 1 and 2 below are also
//! checked then, writing nothing ([`UpdateInstaller::unavailable`]): where
//! they fail, the app offers the way by hand instead of Install. An install
//! checks them again, in case the machine changed since, and then goes, in
//! order:
//!
//! | Step | Fails as |
//! |---|---|
//! | 1. the app runs from a bundle, not from a copy macOS translocated (`…/AppTranslocation/…`, an app opened from its disk image or the Downloads folder without being moved) | [`InstallError::Unavailable`] |
//! | 2. this user may write to the bundle's directory (`access(2)`) | [`InstallError::Unavailable`] |
//! | 3. the download, a regular file and not a symlink, is copied into a private working directory and the copy's sha256 checked against the digest it was downloaded with | [`InstallError::Rejected`]; the copy not written (the working directory full or out of reach): [`InstallError::Failed`] |
//! | 4. the copy is mounted read-only and privately: `hdiutil attach -nobrowse -readonly -noautoopen -mountpoint <dir> <copy>` | missing `hdiutil`: [`InstallError::Unavailable`]; failing: [`InstallError::Failed`] |
//! | 5. the image holds exactly one app at its root, a `Delta.app` whose `Info.plist` states `CFBundleIdentifier` `io.github.x7c1.delta` and `CFBundleShortVersionString` the requested version | [`InstallError::Rejected`] |
//! | 6. that `Delta.app` is copied next to the running one, as `.delta-update-new`, with `ditto` (keeping symlinks, permissions and extended attributes, so the code signature holds) | [`InstallError::Failed`] |
//! | 7. the image is detached (`hdiutil detach`), on every path once step 4 attached it | logged |
//! | 8. the running bundle is renamed to `.delta-update-backup` next to it, and the new one to the bundle's name; if the second rename fails, the first is undone | [`InstallError::Failed`] |
//!
//! The running app's files are never written to — overwriting a running
//! binary in place breaks its signature and the kernel kills it — and it
//! keeps running from the backup until it exits. The app to restart into is
//! the bundle found at startup ([`InstalledApp::Bundle`]); the next launch
//! removes the backup ([`BundleInstaller::remove_update_leftovers`]).
//!
//! [`UpdateInstaller`]: delta_usecase::UpdateInstaller
//! [`UpdateInstaller::unavailable`]: delta_usecase::UpdateInstaller::unavailable
//! [`InstallError`]: delta_usecase::InstallError
//! [`InstallError::Dismissed`]: delta_usecase::InstallError::Dismissed
//! [`InstallError::Unavailable`]: delta_usecase::InstallError::Unavailable
//! [`InstallError::Unrunnable`]: delta_usecase::InstallError::Unrunnable
//! [`InstallError::Rejected`]: delta_usecase::InstallError::Rejected
//! [`InstallError::Failed`]: delta_usecase::InstallError::Failed
//! [`InstalledApp::Bundle`]: delta_usecase::InstalledApp::Bundle

mod bundle_installer;
pub use bundle_installer::BundleInstaller;
mod bundle_location;
use bundle_location::BundleLocation;
mod disk_image_tools;
use disk_image_tools::DiskImageTools;
mod image_check;
mod pkexec_installer;
pub use pkexec_installer::PkexecInstaller;
mod swap_paths;
mod system_disk_image_tools;
use system_disk_image_tools::SystemDiskImageTools;
#[cfg(test)]
mod testing;
mod tool_error;
use tool_error::ToolError;
mod verified_copy;

/// Where the `.deb` installs Delta's update helper, root-owned: the one path
/// the polkit action allows (`org.freedesktop.policykit.exec.path`).
pub const UPDATE_HELPER_PATH: &str = "/usr/lib/delta-desktop/delta-update-helper";

/// `pkexec`, by absolute path, so nothing on the user's `PATH` stands in for
/// it.
pub const PKEXEC_PATH: &str = "/usr/bin/pkexec";

/// Where the `.deb` installs the app: what restarting into an installed
/// update starts on Linux.
pub const INSTALLED_APP_PATH: &str = "/usr/bin/delta-desktop";
