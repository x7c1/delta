use std::ffi::OsString;

use super::{locale_fallback, login_shell, read_login_shell_env, LoginEnv, PathNotImported};

/// Read the login shell's `PATH` and locale, or nothing (with a warning) when
/// they cannot be read, and add a UTF-8 `LANG` when no locale is set either
/// way.
///
/// Changes nothing in the process environment: it reads `SHELL`, the inherited
/// locale (to decide the fallback) and the inherited `PATH` (for the report),
/// and returns what the commands Delta starts should be given.
pub fn read_login_env() -> LoginEnv {
    read_login_env_from(login_shell(std::env::var_os("SHELL")))
}

/// [`read_login_env`] against a given `shell`.
fn read_login_env_from(shell: OsString) -> LoginEnv {
    let printed = read_login_shell_env(&shell);
    login_env_from(shell, printed, |name| std::env::var_os(name))
}

/// The [`LoginEnv`] for what `shell` printed (or why it printed nothing), with
/// `inherited` reading the environment the app was started with.
fn login_env_from(
    shell: OsString,
    printed: Result<Vec<(String, String)>, String>,
    inherited: impl Fn(&str) -> Option<OsString>,
) -> LoginEnv {
    let not_imported = |reason: String| PathNotImported {
        shell: shell.clone(),
        reason,
        inherited_path: inherited("PATH"),
    };
    let (mut vars, path_not_imported) = match printed {
        Ok(vars) => {
            let printed_path = vars.iter().any(|(name, _)| name == "PATH");
            if !printed_path {
                tracing::warn!(
                    shell = %shell.to_string_lossy(),
                    "the login shell printed no PATH; keeping the inherited one"
                );
            }
            let names: Vec<&str> = vars.iter().map(|(name, _)| name.as_str()).collect();
            tracing::info!(
                shell = %shell.to_string_lossy(),
                vars = ?names,
                "using the login shell's environment"
            );
            let path_not_imported =
                (!printed_path).then(|| not_imported("it printed no PATH".to_owned()));
            (vars, path_not_imported)
        }
        Err(reason) => {
            tracing::warn!(
                shell = %shell.to_string_lossy(),
                "could not read the login shell's environment ({reason}); keeping the inherited one"
            );
            (Vec::new(), Some(not_imported(reason)))
        }
    };
    // The locale a child ends up with: the shell's value where it printed one,
    // the inherited one otherwise.
    let effective = |name: &str| {
        vars.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| OsString::from(value))
            .or_else(|| inherited(name))
    };
    if let Some((name, value)) = locale_fallback(effective) {
        tracing::info!("no locale is set; using {name}={value}");
        vars.push((name.to_owned(), value.to_owned()));
    }
    LoginEnv {
        vars,
        path_not_imported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn inherited_from<'a>(
        pairs: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn the_shells_pairs_are_returned_as_printed() {
        let printed = pairs(&[
            ("PATH", "/opt/homebrew/bin:/usr/bin"),
            ("LANG", "ja_JP.UTF-8"),
        ]);

        let env = login_env_from(
            "/bin/zsh".into(),
            Ok(printed.clone()),
            inherited_from(&[("PATH", "/usr/bin:/bin")]),
        );

        assert_eq!(env.vars, printed);
        assert!(env.path_not_imported.is_none());
    }

    #[test]
    fn the_locale_fallback_is_added_to_the_pairs_when_no_locale_decides_the_charset() {
        let env = login_env_from(
            "/bin/zsh".into(),
            Ok(pairs(&[("PATH", "/a"), ("LC_TIME", "C")])),
            inherited_from(&[("LANG", "")]),
        );

        assert_eq!(
            env.vars,
            pairs(&[("PATH", "/a"), ("LC_TIME", "C"), ("LANG", "en_US.UTF-8")])
        );
    }

    #[test]
    fn an_inherited_locale_needs_no_fallback() {
        let env = login_env_from(
            "/bin/zsh".into(),
            Ok(pairs(&[("PATH", "/a")])),
            inherited_from(&[("LC_ALL", "ja_JP.UTF-8")]),
        );

        assert_eq!(env.vars, pairs(&[("PATH", "/a")]));
    }

    #[test]
    fn an_unread_shell_yields_only_the_fallback_and_the_reason() {
        let env = login_env_from(
            "/bin/zsh".into(),
            Err("no answer within 8s".to_owned()),
            inherited_from(&[("PATH", "/usr/bin:/bin")]),
        );

        assert_eq!(env.vars, pairs(&[("LANG", "en_US.UTF-8")]));
        let not_imported = env.path_not_imported.expect("the PATH was not read");
        assert_eq!(not_imported.shell, "/bin/zsh");
        assert_eq!(not_imported.reason, "no answer within 8s");
        assert_eq!(not_imported.inherited_path, Some("/usr/bin:/bin".into()));
    }

    #[test]
    fn a_shell_that_prints_no_path_is_reported() {
        let env = login_env_from(
            "/bin/zsh".into(),
            Ok(pairs(&[("LANG", "C.UTF-8")])),
            inherited_from(&[]),
        );

        assert_eq!(env.vars, pairs(&[("LANG", "C.UTF-8")]));
        let not_imported = env.path_not_imported.expect("the PATH was not printed");
        assert_eq!(not_imported.reason, "it printed no PATH");
        assert_eq!(not_imported.inherited_path, None);
    }

    /// End to end against `/bin/sh`: the shell's `PATH` comes back as a pair,
    /// and the process environment is exactly what it was.
    #[test]
    fn reading_a_real_shell_leaves_the_process_environment_untouched() {
        let before: Vec<_> = std::env::vars_os().collect();

        let env = read_login_env_from("/bin/sh".into());

        let after: Vec<_> = std::env::vars_os().collect();
        assert_eq!(before, after);
        assert!(env
            .vars
            .iter()
            .any(|(name, value)| name == "PATH" && !value.is_empty()));
        assert!(env.path_not_imported.is_none());
    }
}
