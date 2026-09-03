//! Confinement for the transcript path a Claude Code hook reports.

use std::path::{Component, Path};

use crate::error::{Error, Result};

/// Require `transcript_path` to be a `.jsonl` file resolving under `root`.
///
/// The transcript is read from disk and its parsed lines are surfaced to the
/// browser, so an unconfined path is a file-disclosure primitive: a forged (or
/// the first, previously-unvalidated) hook could name any readable file. Three
/// checks close that:
///
/// - the path must be absolute and end in `.jsonl` (real Claude Code
///   transcripts always do),
/// - it must contain no `..` component at all, rejected *lexically* so a
///   traversal cannot hide behind a directory that does not exist yet, and
/// - it must resolve *under* `root` once symlinks are collapsed.
///
/// Nothing about the target is required to exist. Claude Code writes the
/// transcript to `<config>/projects/<cwd-slug>/<id>.jsonl` and creates the
/// per-project directory lazily, on the first transcript write — which happens
/// *after* the `SessionStart` hook has fired. So for a cwd Claude Code has
/// never run in before, neither the file nor its parent directory exists when
/// this runs, and requiring the parent would wedge every first launch in a
/// fresh worktree. Instead the deepest *existing* ancestor is canonicalized
/// (this is what collapses a symlinked prefix such as macOS's `/tmp` →
/// `/private/tmp`) and the not-yet-existing tail is re-joined onto it. That is
/// sound because a component that is genuinely not on disk cannot redirect
/// anywhere, and the lexical `..` rejection above means the tail cannot climb
/// back out. The one component `canonicalize` *calls* absent while it really
/// exists — a dangling symlink, whose target whoever creates it later gets to
/// choose — is refused explicitly below. `root` is canonicalized too, so a
/// symlinked root still matches, and `Path::starts_with` compares whole path
/// components, so a sibling like `<root>x` cannot slip past a string-prefix
/// match.
pub(in crate::interactor::hooks) fn validate_transcript_path(
    root: &str,
    transcript_path: &str,
) -> Result<()> {
    let reject = |reason: &str| {
        Err(Error::InvalidTranscriptPath(format!(
            "{transcript_path}: {reason}"
        )))
    };

    let path = Path::new(transcript_path);
    if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
        return reject("must be a .jsonl file");
    }
    if !path.is_absolute() {
        return reject("must be an absolute path");
    }
    // `.` needs no arm of its own: `Components` normalizes it away, and the one
    // spelling it keeps — a leading `./` — is already out on the absolute check.
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return reject("must not contain a `..` component");
    }

    let canonical_root = match std::fs::canonicalize(root) {
        Ok(root) => root,
        Err(err) => {
            return reject(&format!("transcript root {root} is unresolvable: {err}"));
        }
    };

    // Walk up until something on disk answers, collecting the missing tail on
    // the way. `ancestors()` starts at the path itself — so an already-created
    // transcript is canonicalized directly, symlinked file included — and ends
    // at `/`, which always resolves, so the loop is guaranteed to terminate on
    // one of the two arms below.
    let mut missing_tail = Vec::new();
    let mut resolved = None;
    for ancestor in path.ancestors() {
        match std::fs::canonicalize(ancestor) {
            Ok(canonical) => {
                resolved = Some(canonical);
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // `canonicalize` also reports a *dangling* symlink as absent,
                // and that one is not safe to keep: it is a real component
                // whose target is picked later, by whoever creates it, so it
                // could redirect the finished path anywhere. Only a component
                // that is genuinely not on disk may join the missing tail.
                if std::fs::symlink_metadata(ancestor).is_ok() {
                    return reject(&format!(
                        "ancestor {} is a symlink to a path that does not exist",
                        ancestor.display()
                    ));
                }
                // Not there yet: remember the name and keep climbing.
                if let Some(name) = ancestor.file_name() {
                    missing_tail.push(name.to_owned());
                }
            }
            Err(err) => {
                // Anything other than "absent" (a non-directory in the middle
                // of the path, an unreadable directory) is not a shape a real
                // transcript path has; refuse rather than guess.
                return reject(&format!(
                    "ancestor {} is unresolvable: {err}",
                    ancestor.display()
                ));
            }
        }
    }
    let Some(mut resolved) = resolved else {
        return reject("has no resolvable ancestor");
    };
    // Deepest-first while climbing, so re-join in reverse.
    for name in missing_tail.iter().rev() {
        resolved.push(name);
    }

    if !resolved.starts_with(&canonical_root) {
        return reject(&format!(
            "resolves to {} outside the allowed root {}",
            resolved.display(),
            canonical_root.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_transcript_path;

    /// A `.jsonl` file directly under the root is accepted, even when it does not
    /// exist yet — the root is the only thing that has to resolve.
    #[test]
    fn accepts_a_jsonl_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let path = dir.path().join("not-created-yet.jsonl");
        assert!(validate_transcript_path(root, path.to_str().unwrap()).is_ok());
    }

    /// The shape a first-ever launch produces: Claude Code creates the
    /// per-project directory lazily, on the first transcript write, which comes
    /// *after* `SessionStart` — so at hook time neither the file nor its parent
    /// exists yet. Confinement must still accept it.
    #[test]
    fn accepts_a_jsonl_whose_parent_directory_does_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let path = dir.path().join("new-project/session.jsonl");
        assert!(!path.parent().unwrap().exists());
        assert!(validate_transcript_path(root, path.to_str().unwrap()).is_ok());

        // Several missing levels are fine too: nothing on the missing tail is
        // on disk at all, so only the existing prefix has to be resolved.
        let deeper = dir.path().join("a/b/c/session.jsonl");
        assert!(validate_transcript_path(root, deeper.to_str().unwrap()).is_ok());
    }

    /// A missing directory is not a hiding place for a traversal: `..` is
    /// refused lexically, so it cannot be smuggled through a parent that does
    /// not exist (and so escapes `canonicalize`).
    #[test]
    fn rejects_a_dotdot_escape_through_a_missing_parent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.to_str().unwrap();

        let escape = format!("{root}/missing/../../secret.jsonl");
        assert!(validate_transcript_path(root, &escape).is_err());

        // And a sibling of the root reached through a directory that does not
        // exist yet is refused on the resolved-prefix check.
        let sibling = dir.path().join("not-created-yet/secret.jsonl");
        assert!(validate_transcript_path(root, sibling.to_str().unwrap()).is_err());
    }

    /// A dangling symlink is the one component `canonicalize` calls absent while
    /// it is very much there — so admitting it to the missing tail would let it
    /// stand in for a directory whose target is chosen *later*, by whoever
    /// creates it, root or no root. Refuse it.
    #[test]
    fn rejects_a_dangling_symlink_on_the_missing_tail() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        std::fs::create_dir_all(&root).unwrap();
        // The link resolves nowhere *yet*: the target is outside the root, and
        // it does not exist, so `canonicalize` reports the link as NotFound.
        let outside = dir.path().join("outside");
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        let path = root.join("link/session.jsonl");
        assert!(validate_transcript_path(root.to_str().unwrap(), path.to_str().unwrap()).is_err());
    }

    /// A symlinked root (macOS's `/tmp` → `/private/tmp` in miniature) still
    /// accepts a transcript that has not been created yet: both sides are
    /// resolved, so the prefix comparison is made between real paths.
    #[test]
    fn accepts_a_missing_transcript_under_a_symlinked_root() {
        let dir = tempfile::tempdir().unwrap();
        let real_root = dir.path().join("real");
        std::fs::create_dir_all(&real_root).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real_root, &link).unwrap();

        let path = link.join("new-project/session.jsonl");
        assert!(validate_transcript_path(link.to_str().unwrap(), path.to_str().unwrap()).is_ok());
        // The real root accepts the symlinked spelling as well, since the path
        // is resolved before the comparison.
        assert!(
            validate_transcript_path(real_root.to_str().unwrap(), path.to_str().unwrap()).is_ok()
        );
    }

    /// A relative path is refused: it would otherwise be resolved against the
    /// server's cwd, which is not the transcript root.
    #[test]
    fn rejects_a_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        assert!(validate_transcript_path(root, "t.jsonl").is_err());
        assert!(validate_transcript_path(root, "./t.jsonl").is_err());
    }

    /// A path outside the root is refused, and so is a `..` escape that would
    /// otherwise resolve above it.
    #[test]
    fn rejects_paths_outside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.to_str().unwrap();

        // A sibling directory of the root.
        let outside = dir.path().join("secret.jsonl");
        assert!(validate_transcript_path(root, outside.to_str().unwrap()).is_err());

        // A `..` traversal from inside the root back out to the sibling.
        let escape = dir.path().join("projects/../secret.jsonl");
        assert!(validate_transcript_path(root, escape.to_str().unwrap()).is_err());
    }

    /// A non-`.jsonl` target is refused even under the root, so a hook cannot name
    /// an arbitrary readable file that merely happens to live there.
    #[test]
    fn rejects_a_non_jsonl_extension() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let path = dir.path().join("id_rsa");
        assert!(validate_transcript_path(root, path.to_str().unwrap()).is_err());
    }
}
