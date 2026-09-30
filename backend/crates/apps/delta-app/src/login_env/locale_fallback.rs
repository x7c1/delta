use std::ffi::OsString;

/// The `LANG` set when neither the app nor the login shell sets a locale.
pub const FALLBACK_LANG: &str = "en_US.UTF-8";

/// The variables that decide the character set, in POSIX precedence order: the
/// first one set (and non-empty) wins.
const CHARSET_LOCALE_VARS: [&str; 3] = ["LC_ALL", "LC_CTYPE", "LANG"];

/// The variable to set so the process has a UTF-8 locale, or `None` when one
/// of [`CHARSET_LOCALE_VARS`] is already set. `lookup` reads a variable; an
/// empty value counts as unset, as it does for the C library.
pub fn locale_fallback(
    lookup: impl Fn(&str) -> Option<OsString>,
) -> Option<(&'static str, &'static str)> {
    let has_locale = CHARSET_LOCALE_VARS
        .iter()
        .any(|name| lookup(name).is_some_and(|value| !value.is_empty()));
    (!has_locale).then_some(("LANG", FALLBACK_LANG))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup_in<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn the_fallback_sets_a_utf8_lang_when_no_locale_is_set() {
        assert_eq!(
            locale_fallback(lookup_in(&[])),
            Some(("LANG", "en_US.UTF-8"))
        );
        // Empty values and categories that do not decide the charset do not count.
        assert_eq!(
            locale_fallback(lookup_in(&[("LANG", ""), ("LC_TIME", "C")])),
            Some(("LANG", "en_US.UTF-8"))
        );
    }

    #[test]
    fn the_fallback_leaves_any_charset_locale_alone() {
        for name in ["LC_ALL", "LC_CTYPE", "LANG"] {
            assert_eq!(locale_fallback(lookup_in(&[(name, "ja_JP.UTF-8")])), None);
        }
    }
}
