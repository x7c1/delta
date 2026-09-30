//! `expand_leading_tilde` mirrors the shell's leading-tilde rules.

use crate::interactor::launch_options::expand_leading_tilde;

/// A value of exactly `~` expands to the home directory.
#[test]
fn expand_leading_tilde_bare_tilde_becomes_home() {
    assert_eq!(expand_leading_tilde("~", Some("/home/u")), "/home/u");
}

/// A `~/`-prefixed value has its `~` replaced by home.
#[test]
fn expand_leading_tilde_prefixed_value_uses_home() {
    assert_eq!(
        expand_leading_tilde("~/repos/x", Some("/home/u")),
        "/home/u/repos/x"
    );
}

/// A trailing slash on home does not produce a double slash.
#[test]
fn expand_leading_tilde_trims_trailing_slash_on_home() {
    assert_eq!(expand_leading_tilde("~/x", Some("/home/u/")), "/home/u/x");
}

/// An absolute path is left unchanged.
#[test]
fn expand_leading_tilde_leaves_absolute_path_unchanged() {
    assert_eq!(expand_leading_tilde("/opt/p", Some("/home/u")), "/opt/p");
}

/// A plain non-path value passes through untouched.
#[test]
fn expand_leading_tilde_leaves_plain_value_unchanged() {
    assert_eq!(expand_leading_tilde("auto", Some("/home/u")), "auto");
}

/// `~user` (tilde-user, not a leading `~/`) is not expanded.
#[test]
fn expand_leading_tilde_leaves_tilde_user_unchanged() {
    assert_eq!(expand_leading_tilde("~user/x", Some("/home/u")), "~user/x");
}

/// An embedded (non-leading) `~` is not expanded.
#[test]
fn expand_leading_tilde_leaves_embedded_tilde_unchanged() {
    assert_eq!(
        expand_leading_tilde("/opt/~/x", Some("/home/u")),
        "/opt/~/x"
    );
}

/// With no home (HOME unset) the value is left as-is rather than failing.
#[test]
fn expand_leading_tilde_without_home_leaves_value_unchanged() {
    assert_eq!(expand_leading_tilde("~/x", None), "~/x");
}
