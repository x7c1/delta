//! Repository identity for the new-session Repository tab.
//!
//! A Repository bundles one or more local clones of the same upstream under a
//! single recency-ordered entry. The identity that bundles them is derived
//! from the `origin` URL (when set), normalised so the same upstream reached
//! over SSH and HTTPS — or with different casing on the host — collapses to
//! one key. When `origin` is unset the clone stands alone, keyed by its
//! absolute path.

use crate::ports::WorktreeStartPoint;

/// A registered repository: identity, display name, the clone to default to,
/// and the per-clone state for every clone known to belong to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// Stable identity used to bundle clones. Either the normalised origin URL
    /// (when set on every clone) or — when no `origin` was found — the clone's
    /// own absolute path verbatim.
    pub identity_key: String,
    /// Human-readable name derived from `identity_key` (e.g. `x7c1/delta`) or,
    /// when no `org/repo` segment can be recovered, the basename of the
    /// recently-used clone path.
    pub display_name: String,
    /// The default clone to pre-select: the one with the most recent
    /// activity across this repository's clones.
    pub recently_used_clone_path: String,
    /// All known clones for this repository, ordered most-recent first.
    pub clones: Vec<Clone>,
}

/// One local clone of a repository: its absolute path and the per-clone state
/// derived from the session history at that path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clone {
    /// Absolute path of the clone (the dir the user picked at spawn time).
    pub path: String,
    /// Timestamp of the most recent activity at this clone (`MAX` of the
    /// sessions' `last_activity_at`/`created_at`), ISO-8601 UTC. `None` only
    /// when every contributing session is itself activity-less.
    pub last_opened_at: Option<String>,
    /// Local branch checked out by the most recent session at this clone
    /// (its `branch_at_launch`). `None` for sessions that recorded none (not
    /// a git repo, detached HEAD, or pre-dating the snapshot column).
    pub last_branch: Option<String>,
    /// Launch-option ids selected for the most recent session at this clone.
    /// Phase B always returns `[]` — per-session launch-option persistence is
    /// deferred to a follow-up PR (see `Repository tab` plan).
    pub last_launch_option_ids: Vec<i64>,
    /// Whether the most recent session at this clone opted into a worktree.
    /// Phase B always returns `false` — per-session worktree-state persistence
    /// is deferred to a follow-up PR.
    pub last_worktree_enabled: bool,
    /// The most recent session's worktree start point, when one was chosen.
    /// Phase B always returns `None` — per-session worktree-state persistence
    /// is deferred to a follow-up PR.
    pub last_worktree_start_point: Option<WorktreeStartPoint>,
}

/// Derive a stable identity key for the repository containing `fallback_path`.
///
/// When `origin` is `Some`, the URL is normalised so SSH and HTTPS forms of
/// the same upstream collapse to one key: trailing `.git` is stripped, an
/// SSH `git@host:org/repo` URL is rewritten to `host/org/repo`, an HTTPS
/// `https://host/org/repo` URL is rewritten to `host/org/repo` (with any
/// embedded `user:token@` credentials stripped), and the host portion is
/// lowercased while path segments stay case-sensitive (Git semantics).
///
/// When `origin` is `None` the function falls back to `fallback_path`
/// verbatim — the clone stands alone in the Repository tab.
pub fn identity_key(origin: Option<String>, fallback_path: &str) -> String {
    let Some(raw) = origin else {
        return fallback_path.to_owned();
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return fallback_path.to_owned();
    }
    // Strip a trailing `.git` once.
    let trimmed = raw.strip_suffix(".git").unwrap_or(raw);

    // Scheme-bearing URLs come first so the SSH `user@host:path` branch below
    // does not mis-parse `https:` as a `user@host` separator.
    for scheme in ["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = trimmed.strip_prefix(scheme) {
            // Drop any embedded `user[:token]@` credentials so the same URL
            // with and without a token collapses to one key.
            let rest = rest
                .rsplit_once('@')
                .map(|(_, after)| after)
                .unwrap_or(rest);
            let (host, path) = match rest.split_once('/') {
                Some((host, path)) => (host, path),
                None => (rest, ""),
            };
            if path.is_empty() {
                return host.to_ascii_lowercase();
            }
            return format!("{}/{}", host.to_ascii_lowercase(), path);
        }
    }

    // SSH form: `git@host:org/repo` (or any `user@host:path`).
    if let Some((user_host, rest)) = trimmed.split_once(':') {
        if !user_host.contains('/') {
            let host = user_host
                .rsplit_once('@')
                .map(|(_, h)| h)
                .unwrap_or(user_host);
            return format!("{}/{}", host.to_ascii_lowercase(), rest);
        }
    }

    // Unknown form: keep verbatim (after the `.git` strip) — the caller
    // bundles by exact match in that case.
    trimmed.to_owned()
}

/// Derive a human-readable display name from a `identity_key`.
///
/// When the key looks like `host/org/repo` (or `host/org/sub/repo`), the
/// trailing two segments (`org/repo`) become the display name. Otherwise the
/// basename of `fallback_path` is used (e.g. for a key that *is* a filesystem
/// path because `origin` was unset).
pub fn display_name(identity_key: &str, fallback_path: &str) -> String {
    // A normalised origin key starts with a non-empty host segment (e.g.
    // `github.com/org/repo`); a path key starts with an empty segment
    // (the leading `/`), so split-on-`/` is enough to tell them apart
    // without baking host-shape heuristics in.
    let segments: Vec<&str> = identity_key.split('/').collect();
    let is_origin_shaped =
        segments.first().map(|s| !s.is_empty()).unwrap_or(false) && segments.len() >= 3;
    if is_origin_shaped {
        // `host/org/repo` or deeper: take the last two segments as `org/repo`.
        let n = segments.len();
        return format!("{}/{}", segments[n - 2], segments[n - 1]);
    }
    // Fallback: the basename of the fallback path (skip empty trailing slash).
    fallback_path
        .trim_end_matches('/')
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(fallback_path)
        .to_owned()
}

/// Slugify a [`display_name`] result for use as a filesystem path segment.
///
/// Replaces `/` with `-` (so `org/repo` becomes `org-repo`) and replaces any
/// character outside `[A-Za-z0-9._-]` with `_`. Returns the input unchanged
/// when it already consists only of safe characters.
///
/// Used to build the per-session worktree directory name
/// (`<base>/<slug>-<session-id>`), so a listing of `$DELTA_WORKTREE_BASE`
/// makes each worktree distinguishable at a glance instead of showing a
/// wall of UUID-suffixed `delta-<id>` entries.
pub fn worktree_dir_slug(display_name: &str) -> String {
    display_name
        .chars()
        .map(|c| match c {
            '/' => '-',
            c if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') => c,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_url_collapses_to_host_org_repo() {
        assert_eq!(
            identity_key(Some("https://github.com/x7c1/delta.git".into()), "/x"),
            "github.com/x7c1/delta"
        );
    }

    #[test]
    fn ssh_url_collapses_to_host_org_repo() {
        assert_eq!(
            identity_key(Some("git@github.com:x7c1/delta".into()), "/x"),
            "github.com/x7c1/delta"
        );
    }

    #[test]
    fn ssh_and_https_collapse_to_the_same_key() {
        let ssh = identity_key(Some("git@github.com:x7c1/delta.git".into()), "/a");
        let https = identity_key(Some("https://github.com/x7c1/delta".into()), "/b");
        assert_eq!(ssh, https);
    }

    #[test]
    fn host_case_is_normalised_but_path_case_is_preserved() {
        assert_eq!(
            identity_key(Some("https://GitHub.com/X7c1/Delta".into()), "/a"),
            "github.com/X7c1/Delta"
        );
    }

    #[test]
    fn embedded_credentials_are_stripped() {
        assert_eq!(
            identity_key(
                Some("https://oauth2:token@github.com/x7c1/delta.git".into()),
                "/x"
            ),
            "github.com/x7c1/delta"
        );
    }

    #[test]
    fn none_origin_falls_back_to_path() {
        assert_eq!(identity_key(None, "/projects/scratch"), "/projects/scratch");
    }

    #[test]
    fn empty_origin_falls_back_to_path() {
        assert_eq!(
            identity_key(Some("   ".into()), "/projects/scratch"),
            "/projects/scratch"
        );
    }

    #[test]
    fn display_name_uses_org_repo_when_present() {
        assert_eq!(display_name("github.com/x7c1/delta", "/path"), "x7c1/delta");
    }

    #[test]
    fn display_name_falls_back_to_basename_for_a_path_key() {
        assert_eq!(
            display_name("/projects/scratch", "/projects/scratch"),
            "scratch"
        );
    }

    #[test]
    fn worktree_dir_slug_rewrites_slash_in_org_repo() {
        assert_eq!(worktree_dir_slug("x7c1/delta"), "x7c1-delta");
    }

    #[test]
    fn worktree_dir_slug_leaves_a_safe_basename_unchanged() {
        assert_eq!(worktree_dir_slug("delta"), "delta");
    }

    #[test]
    fn worktree_dir_slug_rewrites_every_slash() {
        assert_eq!(worktree_dir_slug("org/sub/repo"), "org-sub-repo");
    }

    #[test]
    fn worktree_dir_slug_sanitizes_unsafe_characters() {
        assert_eq!(worktree_dir_slug("foo bar!baz"), "foo_bar_baz");
    }

    #[test]
    fn worktree_dir_slug_passes_empty_through() {
        assert_eq!(worktree_dir_slug(""), "");
    }
}
