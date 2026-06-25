//! Repository identity helpers: turning a remote origin URL into a stable
//! `host/org/repo` identity key and a short `org/repo` display name.
//!
//! Two repositories are "the same" when their identity keys match. The key is
//! deliberately origin-shaped (`host/org/repo`) so an SSH clone
//! (`git@github.com:org/repo.git`) and an HTTPS clone
//! (`https://github.com/org/repo.git`) collide on the same key — they are the
//! same repository. When the launch directory has no origin URL configured,
//! the key falls back to the path itself so distinct local-only repos still
//! get distinct identities.
//!
//! These helpers are pure — they do not shell out — so they live in the
//! use-case crate next to the [`crate::GitWorktree`] port that produces the
//! raw origin URL string. The gateway is responsible only for `git config
//! --get remote.origin.url`; normalising the answer happens here so every
//! caller agrees on the canonical form.
//!
//! [`crate::GitWorktree`]: crate::ports::GitWorktree

/// A canonical, comparable identity for a repository.
///
/// Computed by normalising the remote `origin` URL — strip credentials,
/// lowercase the host, strip a trailing `.git`, take the path segments — into
/// `host/org/repo` form. When the origin URL is missing or unparseable, the
/// `fallback_path` is used as the key instead (so a local-only repo still has
/// a stable identity, distinct from any other path).
///
/// The output is the bare key string, suitable for equality comparison and
/// for feeding into [`display_name`] to render a short human-friendly label.
pub fn identity_key(origin_url: Option<String>, fallback_path: &str) -> String {
    if let Some(url) = origin_url.as_deref() {
        if let Some(key) = normalise_origin(url) {
            return key;
        }
    }
    fallback_path.to_owned()
}

/// A short, human-friendly name for a repository, derived from an
/// [`identity_key`].
///
/// When the key is origin-shaped (`host/org/repo` — exactly three slash
/// segments, the first looking like a host with a dot), return `org/repo`.
/// Otherwise the key is a filesystem path (the [`identity_key`] fallback),
/// and the basename of `fallback_path` (the launch directory the key was
/// derived from) is returned — the same basename the navigator would have
/// rendered without this code path.
pub fn display_name(key: &str, fallback_path: &str) -> String {
    if let Some((_host, org_repo)) = split_origin_key(key) {
        return org_repo.to_owned();
    }
    basename(fallback_path)
}

/// Normalise an origin URL string into a `host/org/repo` key.
///
/// Accepts both forms git emits:
/// - SSH `git@host:org/repo(.git)?`
/// - HTTPS `https://[user[:pass]@]host/org/repo(.git)?` (any scheme that
///   contains `://`, including `http`, `ssh`, `git`).
///
/// Returns `None` when the URL does not parse into at least a host plus two
/// path segments. The host is lowercased; case in the path is preserved (most
/// forges treat `Org/Repo` as the same as `org/repo` on the wire but the
/// canonical capitalisation belongs to the path itself).
fn normalise_origin(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (host, path) = if let Some((_scheme, authority_and_path)) = trimmed.split_once("://") {
        // scheme://[creds@]host[/]path...
        let (authority, raw_path) = match authority_and_path.split_once('/') {
            Some((authority, path)) => (authority, path),
            None => return None,
        };
        // Strip credentials.
        let host_part = match authority.rsplit_once('@') {
            Some((_creds, host)) => host,
            None => authority,
        };
        // Strip a possible `:port`.
        let host_only = host_part
            .split_once(':')
            .map(|(h, _)| h)
            .unwrap_or(host_part);
        if host_only.is_empty() {
            return None;
        }
        (host_only.to_owned(), raw_path.to_owned())
    } else if let Some((authority, raw_path)) = trimmed.split_once(':') {
        // SCP-style SSH: `[user@]host:path`. The split must NOT be a `scheme:`
        // — guard by requiring the right side to look like a path (does not
        // start with `//`), and the left side to be non-empty.
        if raw_path.starts_with("//") || authority.is_empty() {
            return None;
        }
        let host_part = match authority.rsplit_once('@') {
            Some((_user, host)) => host,
            None => authority,
        };
        if host_part.is_empty() {
            return None;
        }
        (host_part.to_owned(), raw_path.to_owned())
    } else {
        return None;
    };

    // Strip a trailing `.git` and any surrounding slashes.
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);

    // Require at least `org/repo`.
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    let org_repo = segments.join("/");

    Some(format!("{}/{}", host.to_lowercase(), org_repo))
}

/// Split an `host/org/repo` identity key into `(host, org_repo)` when it is
/// origin-shaped: at least three slash segments and a host that contains a
/// dot. `None` otherwise (e.g. a filesystem-path fallback key).
fn split_origin_key(key: &str) -> Option<(&str, &str)> {
    let (host, rest) = key.split_once('/')?;
    if !host.contains('.') {
        return None;
    }
    // `rest` must itself contain at least `org/repo`.
    if rest.split('/').filter(|s| !s.is_empty()).count() < 2 {
        return None;
    }
    Some((host, rest))
}

/// The basename of `path`, or the path itself when it has no slash.
/// Trailing slashes are stripped first so `/a/b/` resolves to `b`.
fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((_, last)) => last.to_owned(),
        None => trimmed.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_key_normalises_https_url() {
        let key = identity_key(
            Some("https://github.com/x7c1/delta.git".into()),
            "/anywhere",
        );
        assert_eq!(key, "github.com/x7c1/delta");
    }

    #[test]
    fn identity_key_normalises_ssh_url_to_same_key_as_https() {
        let ssh = identity_key(Some("git@github.com:x7c1/delta.git".into()), "/anywhere");
        let https = identity_key(
            Some("https://github.com/x7c1/delta.git".into()),
            "/anywhere",
        );
        assert_eq!(ssh, https, "SSH and HTTPS clones share one identity");
    }

    #[test]
    fn identity_key_strips_credentials_from_https() {
        let key = identity_key(
            Some("https://user:token@github.com/x7c1/delta.git".into()),
            "/anywhere",
        );
        assert_eq!(key, "github.com/x7c1/delta");
    }

    #[test]
    fn identity_key_lowercases_host_only() {
        // Host is lowercased; path segments preserve their capitalisation.
        let key = identity_key(
            Some("https://GitHub.com/X7C1/Delta.git".into()),
            "/anywhere",
        );
        assert_eq!(key, "github.com/X7C1/Delta");
    }

    #[test]
    fn identity_key_falls_back_to_path_when_origin_missing() {
        let key = identity_key(None, "/work/local-only");
        assert_eq!(key, "/work/local-only");
    }

    #[test]
    fn identity_key_falls_back_to_path_when_origin_is_unparseable() {
        let key = identity_key(Some("not-a-url".into()), "/work/local-only");
        assert_eq!(key, "/work/local-only");
    }

    #[test]
    fn display_name_returns_org_repo_for_origin_key() {
        let key = identity_key(
            Some("https://github.com/x7c1/delta.git".into()),
            "/work/anywhere",
        );
        assert_eq!(display_name(&key, "/work/anywhere"), "x7c1/delta");
    }

    #[test]
    fn display_name_falls_back_to_basename_for_path_key() {
        let key = identity_key(None, "/work/local-only");
        assert_eq!(display_name(&key, "/work/local-only"), "local-only");
    }

    #[test]
    fn display_name_basename_handles_trailing_slash() {
        let key = identity_key(None, "/work/local-only/");
        assert_eq!(display_name(&key, "/work/local-only/"), "local-only");
    }
}
