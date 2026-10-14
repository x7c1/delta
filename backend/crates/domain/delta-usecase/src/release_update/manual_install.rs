//! How the user installs a downloaded update by hand, when Delta cannot.

use std::path::{Path, PathBuf};

/// The way to install the verified download by hand, for the platform it is
/// for.
///
/// The way out whenever Delta cannot install an update itself — including an
/// updater that turns out to be broken — so it depends on nothing but the
/// file: not on Linux's root helper or polkit, not on the installer that
/// replaces the app bundle on macOS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualInstall {
    /// Linux: commands to run in a terminal outside Delta, built by
    /// [`Self::apt_install`].
    ///
    /// Outside Delta, because a Delta session's pane is one the agents on
    /// its tmux socket can type into, and `sudo` is about to be given the
    /// user's password there.
    Command {
        /// The commands that check the file before it is installed, one per
        /// line: `sha256sum <path>`, to compare with the digest on the
        /// release page, and `minisign -Vm <path> -P <release key>`, which
        /// checks it against the signature Delta kept beside it
        /// (`<path>.minisig`) with the release key.
        check: Vec<String>,
        /// The command that installs it, `sudo -k apt install <path>`: `-k`
        /// makes `sudo` ask for the password and leave no cached
        /// authentication behind for anything else in that terminal.
        install: String,
    },
    /// macOS: open the verified disk image at this absolute path, quit
    /// Delta (Finder does not replace an app that is open), drag Delta to
    /// Applications, and start Delta again.
    DiskImage(PathBuf),
}

impl ManualInstall {
    /// The commands that check and install the verified `.deb` at `path`,
    /// each path quoted for a POSIX shell when it needs to be.
    pub fn apt_install(path: &Path) -> Self {
        let path = shell_quoted(&path.to_string_lossy());
        Self::Command {
            check: vec![
                format!("sha256sum {path}"),
                format!("minisign -Vm {path} -P {}", release_signature::PUBLIC_KEY),
            ],
            install: format!("sudo -k apt install {path}"),
        }
    }
}

/// `word` as one word for a POSIX shell: as it is when every character is one
/// a shell takes literally, single-quoted otherwise.
fn shell_quoted(word: &str) -> String {
    let literal = |c: char| c.is_ascii_alphanumeric() || "/._-+=:,@%".contains(c);
    if !word.is_empty() && word.chars().all(literal) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEB: &str =
        "/home/u/.local/share/io.github.x7c1.delta/updates/delta-desktop_0.6.0_amd64.deb";

    fn commands(path: &str) -> (Vec<String>, String) {
        match ManualInstall::apt_install(Path::new(path)) {
            ManualInstall::Command { check, install } => (check, install),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_install_asks_sudo_for_the_password_and_keeps_no_authentication() {
        let (_, install) = commands(DEB);
        assert_eq!(install, format!("sudo -k apt install {DEB}"));
    }

    #[test]
    fn the_file_is_checked_by_digest_and_by_signature_with_the_release_key() {
        let (check, _) = commands(DEB);
        assert_eq!(
            check,
            [
                format!("sha256sum {DEB}"),
                format!("minisign -Vm {DEB} -P {}", release_signature::PUBLIC_KEY),
            ]
        );
    }

    #[test]
    fn a_path_a_shell_would_split_or_expand_is_quoted() {
        let (check, install) = commands("/home/a user/updates/d.deb");
        assert_eq!(install, "sudo -k apt install '/home/a user/updates/d.deb'");
        assert_eq!(check[0], "sha256sum '/home/a user/updates/d.deb'");
        assert!(
            check[1].starts_with("minisign -Vm '/home/a user/updates/d.deb' -P "),
            "{}",
            check[1]
        );
        let (_, install) = commands("/home/o'neil/$HOME/d.deb");
        assert_eq!(
            install,
            r"sudo -k apt install '/home/o'\''neil/$HOME/d.deb'"
        );
    }
}
