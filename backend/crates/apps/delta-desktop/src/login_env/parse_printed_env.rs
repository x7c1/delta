use super::MARKER;

/// The imported variables (`PATH`, `LANG`, `LC_*`) among the NUL-separated
/// `NAME=value` entries printed between the markers in `stdout` (`env -0`), in
/// the order printed, or `None` when the markers are missing. Empty values are
/// skipped.
///
/// Entries are split on NUL, which no variable can hold, so a newline inside a
/// value stays inside that value: a variable whose value holds
/// `\nPATH=/somewhere` is read as that one variable, never as a `PATH`. The
/// newline the opening marker is printed with is not part of the first entry.
///
/// The environment is read up to the *last* marker: a shell that exports `_`
/// sets it to the previous command's last argument, the opening marker, so
/// `env` itself can print `_=<marker>` before the variables that matter.
pub fn parse_printed_env(stdout: &str) -> Option<Vec<(String, String)>> {
    let start = stdout.find(MARKER)? + MARKER.len();
    let rest = &stdout[start..];
    let end = rest.rfind(MARKER)?;
    let printed = &rest[..end];
    let printed = printed.strip_prefix('\n').unwrap_or(printed);
    let vars = printed
        .split('\0')
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, value)| is_imported(name) && !value.is_empty())
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    Some(vars)
}

/// Whether the login shell's value of `name` is adopted.
fn is_imported(name: &str) -> bool {
    name == "PATH" || name == "LANG" || name.starts_with("LC_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn path_and_locale_are_taken_from_one_output() {
        let stdout = format!(
            "{MARKER}\nHOME=/Users/me\0PATH=/opt/homebrew/bin:/usr/bin\0LANG=ja_JP.UTF-8\0\
             LC_CTYPE=UTF-8\0LC_ALL=en_US.UTF-8\0SHELL=/bin/zsh\0{MARKER}"
        );
        assert_eq!(
            parse_printed_env(&stdout),
            Some(vars(&[
                ("PATH", "/opt/homebrew/bin:/usr/bin"),
                ("LANG", "ja_JP.UTF-8"),
                ("LC_CTYPE", "UTF-8"),
                ("LC_ALL", "en_US.UTF-8"),
            ]))
        );
    }

    #[test]
    fn a_newline_inside_a_value_does_not_start_another_variable() {
        let stdout = format!(
            "{MARKER}\nPATH=/usr/bin:/bin\0EVIL=x\nPATH=/evil\nLANG=C\0\
             LC_ALL=en_US.UTF-8\0{MARKER}"
        );
        assert_eq!(
            parse_printed_env(&stdout),
            Some(vars(&[
                ("PATH", "/usr/bin:/bin"),
                ("LC_ALL", "en_US.UTF-8")
            ]))
        );
    }

    #[test]
    fn a_newline_in_the_first_value_stays_in_it_too() {
        // The entry right after the opening marker's newline is read like any
        // other.
        let stdout = format!("{MARKER}\nEVIL=x\nPATH=/evil\0PATH=/a:/b\0{MARKER}");
        assert_eq!(parse_printed_env(&stdout), Some(vars(&[("PATH", "/a:/b")])));
    }

    #[test]
    fn a_marker_printed_inside_the_environment_does_not_end_it() {
        // `_` holds the opening marker when the shell exports it.
        let stdout = format!("{MARKER}\n_={MARKER}\0PATH=/a:/b\0LANG=C.UTF-8\0{MARKER}");
        assert_eq!(
            parse_printed_env(&stdout),
            Some(vars(&[("PATH", "/a:/b"), ("LANG", "C.UTF-8")]))
        );
    }

    #[test]
    fn output_around_the_markers_is_ignored() {
        let stdout =
            format!("Last login: today\nPATH=/x\0\n{MARKER}\nPATH=/a:/b\0{MARKER}PATH=/y\0\n");
        assert_eq!(parse_printed_env(&stdout), Some(vars(&[("PATH", "/a:/b")])));
    }

    #[test]
    fn entries_that_are_not_assignments_and_empty_values_are_skipped() {
        let stdout =
            format!("{MARKER}\nnot an assignment\0LANG=\0LC_TIME=C\0NOTLC_X=1\0\0{MARKER}");
        assert_eq!(parse_printed_env(&stdout), Some(vars(&[("LC_TIME", "C")])));
    }

    #[test]
    fn missing_or_unterminated_markers_yield_none() {
        assert_eq!(parse_printed_env("PATH=/usr/bin:/bin"), None);
        assert_eq!(
            parse_printed_env(&format!("{MARKER}\nPATH=/usr/bin\0")),
            None
        );
    }
}
