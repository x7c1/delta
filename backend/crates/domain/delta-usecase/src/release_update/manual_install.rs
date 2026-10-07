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
    /// Linux: the command that installs the verified `.deb` from the user's
    /// own terminal, `sudo apt install <path>`, the path quoted for a POSIX
    /// shell when it needs to be ([`Self::apt_install`]).
    Command(String),
    /// macOS: open the verified disk image at this absolute path, quit
    /// Delta (Finder does not replace an app that is open), drag Delta to
    /// Applications, and start Delta again.
    DiskImage(PathBuf),
}

impl ManualInstall {
    /// `sudo apt install <path>`, for the verified `.deb` at `path`.
    pub fn apt_install(path: &Path) -> Self {
        Self::Command(format!(
            "sudo apt install {}",
            shell_quoted(&path.to_string_lossy())
        ))
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

    fn command(path: &str) -> String {
        match ManualInstall::apt_install(Path::new(path)) {
            ManualInstall::Command(command) => command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_plain_path_is_given_as_it_is() {
        assert_eq!(
            command(
                "/home/u/.local/share/io.github.x7c1.delta/updates/delta-desktop_0.6.0_amd64.deb"
            ),
            "sudo apt install /home/u/.local/share/io.github.x7c1.delta/updates/delta-desktop_0.6.0_amd64.deb"
        );
    }

    #[test]
    fn a_path_a_shell_would_split_or_expand_is_quoted() {
        assert_eq!(
            command("/home/a user/updates/d.deb"),
            "sudo apt install '/home/a user/updates/d.deb'"
        );
        assert_eq!(
            command("/home/o'neil/$HOME/d.deb"),
            r"sudo apt install '/home/o'\''neil/$HOME/d.deb'"
        );
    }
}
