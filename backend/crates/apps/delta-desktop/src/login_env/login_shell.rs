use std::ffi::OsString;

/// The shell used when `$SHELL` is unset or empty.
const FALLBACK_SHELL: &str = "/bin/sh";

/// The shell to ask: `$SHELL`, or [`FALLBACK_SHELL`] when it is unset or empty.
pub fn login_shell(shell: Option<OsString>) -> OsString {
    shell
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| OsString::from(FALLBACK_SHELL))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_defaults_to_bin_sh() {
        assert_eq!(login_shell(None), "/bin/sh");
        assert_eq!(login_shell(Some(OsString::new())), "/bin/sh");
        assert_eq!(login_shell(Some("/bin/zsh".into())), "/bin/zsh");
    }
}
