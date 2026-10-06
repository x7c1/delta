//! Settings → Storage use cases over the worktree base: listing the
//! directories under it, and removing one a session no longer works in.

mod list_worktree_dirs;
mod remove_worktree_dir;

#[cfg(test)]
mod tests;

use std::path::Path;

/// The path of the directory `name` directly under `base`, spelled the way
/// Delta spells the worktrees it creates there (`<base>/<name>`), so it
/// compares verbatim against the working directories sessions recorded.
fn child_path(base: &str, name: &str) -> String {
    format!("{}/{name}", base.trim_end_matches('/'))
}

/// Whether `path` names a directory directly under `base`, compared by path
/// components as written: `<base>/<name>` for a single plain `<name>`. No
/// `..`, no deeper path, not `base` itself.
fn is_directly_under(base: &str, path: &str) -> bool {
    let path = Path::new(path);
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    let name = name.to_string_lossy();
    parent == Path::new(base.trim_end_matches('/'))
        && name != "."
        && name != ".."
        && child_path(base, &name) == path.to_string_lossy()
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn only_a_single_plain_component_under_the_base_is_directly_under_it() {
        assert!(is_directly_under("/w", "/w/x7c1-delta-1"));
        assert!(is_directly_under("/w/", "/w/x7c1-delta-1"));
        assert!(!is_directly_under("/w", "/w"));
        assert!(!is_directly_under("/w", "/w/a/b"));
        assert!(!is_directly_under("/w", "/w/.."));
        assert!(!is_directly_under("/w", "/w/../etc"));
        assert!(!is_directly_under("/w", "/w-evil/a"));
        assert!(!is_directly_under("/w", "/elsewhere/a"));
        assert!(!is_directly_under("/w", "/w/a/"));
    }
}
