//! The command that installs a downloaded update from the user's own
//! terminal, when Delta cannot.

use std::path::Path;

/// The command the user can run in their own terminal to install the verified
/// `.deb` at `path`: `sudo apt install <path>`, the path quoted for a POSIX
/// shell when it needs to be.
///
/// The way out whenever Delta cannot install an update itself — including an
/// updater that turns out to be broken — so it depends on nothing but the
/// file: not on the helper, not on polkit.
pub fn manual_install_command(path: &Path) -> String {
    format!("sudo apt install {}", shell_quoted(&path.to_string_lossy()))
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

    #[test]
    fn a_plain_path_is_given_as_it_is() {
        assert_eq!(
            manual_install_command(Path::new(
                "/home/u/.local/share/io.github.x7c1.delta/updates/delta-desktop_0.6.0_amd64.deb"
            )),
            "sudo apt install /home/u/.local/share/io.github.x7c1.delta/updates/delta-desktop_0.6.0_amd64.deb"
        );
    }

    #[test]
    fn a_path_a_shell_would_split_or_expand_is_quoted() {
        assert_eq!(
            manual_install_command(Path::new("/home/a user/updates/d.deb")),
            "sudo apt install '/home/a user/updates/d.deb'"
        );
        assert_eq!(
            manual_install_command(Path::new("/home/o'neil/$HOME/d.deb")),
            r"sudo apt install '/home/o'\''neil/$HOME/d.deb'"
        );
    }
}
