//! Launch-option registry use cases: list, create, and delete the custom
//! `claude` CLI flags the user can later multi-select when starting a session.
//!
//! Each operation is a thin pass-through to the [`SessionStore`] port — the
//! registry has no cross-record invariants to enforce — kept together here so
//! the CRUD surface lives in one place.
//!
//! [`SessionStore`]: crate::ports::SessionStore

mod crud;

#[cfg(test)]
mod tests;

/// Expand a leading `~` in a launch-option value to the user's home directory.
///
/// Launch-option values are forwarded to `claude` as argv tokens *without a
/// shell* (the spawn command line is an argv tail, see `spawn_fresh`), so the
/// shell's own tilde expansion never runs. A `--plugin-dir` value of
/// `~/repos/x/plugins` would otherwise reach `claude` as the literal
/// `~/repos/...`, which `claude` resolves relative to the (worktree) cwd —
/// yielding a bogus `<cwd>/~/repos/...` that does not exist. Mirror the shell's
/// tilde rules: a value of exactly `~` becomes `home`, and a `~/`-prefixed
/// value has the `~` replaced by `home`. Everything else — including `~user`
/// and an embedded (non-leading) `~`, neither of which the shell expands here
/// either — is returned unchanged, so non-path values like `auto` or `opus`
/// pass through untouched. When `home` is `None` (HOME unset, only in
/// degenerate environments) the value is left as-is rather than failing the
/// launch.
pub(in crate::interactor) fn expand_leading_tilde(value: &str, home: Option<&str>) -> String {
    let Some(home) = home else {
        return value.to_owned();
    };
    if value == "~" {
        return home.to_owned();
    }
    match value.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
        None => value.to_owned(),
    }
}
