//! Launch-option registry use cases: list, create, and delete the custom agent
//! launch options the user can later multi-select when starting a session, plus
//! resolving a selection at session start ([`resolve`]).
//!
//! Each CRUD operation is a thin pass-through to the [`SessionStore`] port —
//! the registry has no cross-record invariants to enforce — kept together here
//! so the surface lives in one place.
//!
//! [`SessionStore`]: crate::ports::SessionStore

mod crud;
mod resolve;

#[cfg(test)]
mod tests;

/// Expand a leading `~` in a launch-option value to the user's home directory.
///
/// No shell ever runs over a launch-option value: Claude's ride the spawn
/// command line as an argv tail (see `spawn_fresh`) and Codex's ride a
/// JSON-RPC field, so the shell's own tilde expansion never runs for either. A
/// `--plugin-dir` value of `~/repos/x/plugins` would otherwise reach the agent
/// as the literal `~/repos/...`, which it resolves relative to the (worktree)
/// cwd — yielding a bogus `<cwd>/~/repos/...` that does not exist. Mirror the shell's
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
