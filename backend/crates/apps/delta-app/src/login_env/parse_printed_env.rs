use super::MARKER;

/// The imported variables (`PATH`, `LANG`, `LC_*`) among the `NAME=value`
/// lines printed between the markers in `stdout`, in the order printed, or
/// `None` when the markers are missing. Empty values are skipped.
///
/// A line that is not `NAME=value` — a continuation of a multi-line value of
/// some other variable — is skipped too; none of the imported variables holds
/// a newline.
///
/// The environment is read up to the *last* marker: a shell that exports `_`
/// sets it to the previous command's last argument, the opening marker, so
/// `env` itself can print `_=<marker>` before the variables that matter.
pub fn parse_printed_env(stdout: &str) -> Option<Vec<(String, String)>> {
    let start = stdout.find(MARKER)? + MARKER.len();
    let rest = &stdout[start..];
    let end = rest.rfind(MARKER)?;
    let vars = rest[..end]
        .lines()
        .filter_map(|line| line.split_once('='))
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
            "{MARKER}\nHOME=/Users/me\nPATH=/opt/homebrew/bin:/usr/bin\nLANG=ja_JP.UTF-8\n\
             LC_CTYPE=UTF-8\nLC_ALL=en_US.UTF-8\nSHELL=/bin/zsh\n{MARKER}"
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
    fn a_marker_printed_inside_the_environment_does_not_end_it() {
        // `_` holds the opening marker when the shell exports it.
        let stdout = format!("{MARKER}\n_={MARKER}\nPATH=/a:/b\nLANG=C.UTF-8\n{MARKER}");
        assert_eq!(
            parse_printed_env(&stdout),
            Some(vars(&[("PATH", "/a:/b"), ("LANG", "C.UTF-8")]))
        );
    }

    #[test]
    fn output_around_the_markers_is_ignored() {
        let stdout = format!("Last login: today\nPATH=/x\n{MARKER}\nPATH=/a:/b\n{MARKER}PATH=/y\n");
        assert_eq!(parse_printed_env(&stdout), Some(vars(&[("PATH", "/a:/b")])));
    }

    #[test]
    fn lines_that_are_not_assignments_and_empty_values_are_skipped() {
        let stdout = format!(
            "{MARKER}\nMULTI=first line\nsecond line\nLANG=\nLC_TIME=C\nNOTLC_X=1\n{MARKER}"
        );
        assert_eq!(parse_printed_env(&stdout), Some(vars(&[("LC_TIME", "C")])));
    }

    #[test]
    fn missing_or_unterminated_markers_yield_none() {
        assert_eq!(parse_printed_env("PATH=/usr/bin:/bin"), None);
        assert_eq!(parse_printed_env(&format!("{MARKER}\nPATH=/usr/bin")), None);
    }
}
