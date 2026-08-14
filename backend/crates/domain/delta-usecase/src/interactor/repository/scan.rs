//! Depth-1 git-clone scanning for registered clone roots.
//!
//! `<clone_root>/<child>/.git` (file or dir) is the whole test for "this child
//! is a git clone". No recursion, no `git fetch` / `git rev-parse` spawns —
//! the depth-1 explicit-registration design is the entire contract, so the
//! scan stays cheap enough to run on every `GET /api/repositories`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One clone discovered under a clone root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScannedClone {
    /// Absolute path of the child directory (the clone's working tree root).
    pub path: String,
}

/// Scan `root` for direct-child git clones.
///
/// Reads `<root>` once (depth 1) and returns each immediate child whose
/// `<child>/.git` exists — as a directory (normal clone) or a regular file
/// (linked worktree / submodule). The `.git` stat follows symlinks, since a
/// linked worktree's pointer file is the whole signal.
///
/// A missing or unreadable `root` is logged at `warn` and yields an empty
/// vector — a clone root the user has since removed should not fail the
/// repository list. Symlink loops are guarded by canonicalising the root and
/// each child before recording it: a child whose canonical path repeats one
/// already visited within this scan is skipped.
pub(super) async fn scan_one_root(root: &str) -> Vec<ScannedClone> {
    // Canonicalise the root once so a symlink that points back at it from a
    // child is detected by canonical-path equality. A canonicalisation failure
    // is the "root path no longer exists" case — warn and bail.
    let canonical_root = match tokio::fs::canonicalize(root).await {
        Ok(path) => path,
        Err(err) => {
            tracing::warn!(root, error = %err, "clone root could not be resolved");
            return Vec::new();
        }
    };

    let mut read = match tokio::fs::read_dir(&canonical_root).await {
        Ok(read) => read,
        Err(err) => {
            tracing::warn!(root, error = %err, "clone root could not be read");
            return Vec::new();
        }
    };

    let mut visited: HashSet<PathBuf> = HashSet::new();
    visited.insert(canonical_root.clone());

    let mut out = Vec::new();
    loop {
        let entry = match read.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(root, error = %err, "repository scan entry could not be read");
                break;
            }
        };
        let child_path = entry.path();
        let child_canonical = match tokio::fs::canonicalize(&child_path).await {
            Ok(path) => path,
            // A child that cannot be canonicalised (broken symlink, raced
            // delete) is simply not a clone we can register; skip it silently.
            Err(_) => continue,
        };
        // Loop guard: a symlink in `<root>` that points back at the root
        // itself, or at another already-recorded child, would otherwise be
        // recorded as a clone. Since we never recurse below depth 1 there is
        // no deeper traversal to break, but recording a duplicate would still
        // surface a phantom clone in the result.
        if !visited.insert(child_canonical.clone()) {
            continue;
        }
        if has_git_marker(&child_canonical).await {
            out.push(ScannedClone {
                path: child_canonical.to_string_lossy().into_owned(),
            });
        }
    }
    out
}

/// Whether `dir/.git` exists as either a directory or a regular file.
///
/// A normal clone has `.git` as a directory; a linked worktree (`git worktree
/// add`) or a submodule has `.git` as a regular file containing the pointer
/// `gitdir: ...`. Either counts as "this is a git workspace" for the
/// Repository tab.
async fn has_git_marker(dir: &Path) -> bool {
    tokio::fs::metadata(dir.join(".git")).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// Build an empty git-style child (a directory containing a `.git` dir),
    /// returning its absolute path.
    fn make_clone(parent: &Path, name: &str) -> String {
        let child = parent.join(name);
        std::fs::create_dir(&child).unwrap();
        std::fs::create_dir(child.join(".git")).unwrap();
        child.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn mixed_children_only_git_dirs_are_returned() {
        let tmp = tempfile::tempdir().unwrap();
        let _ = make_clone(tmp.path(), "alpha");
        // A plain non-git child.
        std::fs::create_dir(tmp.path().join("beta")).unwrap();
        let _ = make_clone(tmp.path(), "gamma");

        let mut found: Vec<String> = scan_one_root(tmp.path().to_str().unwrap())
            .await
            .into_iter()
            .map(|c| c.path)
            .collect();
        found.sort();

        let canonical = tokio::fs::canonicalize(tmp.path()).await.unwrap();
        let mut expected = vec![
            canonical.join("alpha").to_string_lossy().into_owned(),
            canonical.join("gamma").to_string_lossy().into_owned(),
        ];
        expected.sort();
        assert_eq!(found, expected);
    }

    #[tokio::test]
    async fn dot_git_as_a_file_is_recognised_as_a_clone() {
        // A linked worktree's `.git` is a regular file containing a `gitdir:`
        // pointer rather than a directory; it still counts as a git workspace.
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("linked");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(child.join(".git"), "gitdir: /elsewhere\n").unwrap();

        let found = scan_one_root(tmp.path().to_str().unwrap()).await;
        assert_eq!(found.len(), 1);
        let canonical_child = tokio::fs::canonicalize(&child).await.unwrap();
        assert_eq!(found[0].path, canonical_child.to_string_lossy());
    }

    #[tokio::test]
    async fn missing_root_yields_empty_without_failing() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        let found = scan_one_root(missing.to_str().unwrap()).await;
        assert!(found.is_empty(), "missing root path scans empty");
    }

    #[tokio::test]
    async fn symlink_loop_to_self_does_not_duplicate() {
        // A symlink inside the root that points back at the root would
        // otherwise surface as a phantom "child" whose `.git` is whatever the
        // root happens to contain. The visited-canonical-path guard skips it.
        let tmp = tempfile::tempdir().unwrap();
        let canonical_root = tokio::fs::canonicalize(tmp.path()).await.unwrap();
        let _ = make_clone(tmp.path(), "real");
        symlink(&canonical_root, tmp.path().join("loop")).unwrap();

        let found = scan_one_root(tmp.path().to_str().unwrap()).await;
        assert_eq!(found.len(), 1, "the self-loop child is skipped");
        assert_eq!(found[0].path, canonical_root.join("real").to_string_lossy());
    }
}
