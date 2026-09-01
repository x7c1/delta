use std::path::Path;

use delta_model::{Session, SessionId, ThreadId};

use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{
    GitWorktree, NewSession, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace,
};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Upsert the session row and emit [`SessionEvent::SessionRegistered`],
    /// returning the session and its `main` thread id.
    ///
    /// For a Delta launch the row already exists as `spawning` (written when the
    /// id was minted), so this *activates* it — `spawning` → `active`, filling
    /// in the hook-reported transcript path; for an external `claude` the row is
    /// inserted fresh (see [`SessionStore::register_session`]).
    ///
    /// Takes the raw identifying fields rather than a specific hook type, because
    /// two hooks register a session — the first `UserPromptSubmit` and
    /// `SessionStart(source=startup)` — and both carry `session_id`, `cwd`, and
    /// `transcript_path`. `register_session` is idempotent for an already-active
    /// id, so a second call is harmless; the event is still emitted (the browser
    /// already invalidates its list idempotently on it).
    pub(in crate::interactor::hooks) async fn register_session_row(
        &self,
        session_id: &SessionId,
        cwd: &str,
        transcript_path: &str,
        events: &mut Vec<SessionEvent>,
    ) -> Result<(Session, ThreadId)> {
        // Confine the hook-reported transcript path before it is persisted. This
        // is the single choke point every registering hook (the first
        // `UserPromptSubmit` and `SessionStart(startup)`) funnels through, so a
        // path Delta refuses here is never stored and never reaches the tailer's
        // `fs::read_to_string`.
        if let Some(root) = &self.transcript_root {
            validate_transcript_path(root, transcript_path)?;
        }
        let (session, main_id) = self
            .store
            .register_session(NewSession {
                id: session_id.clone(),
                cwd: cwd.to_owned(),
                transcript_path: transcript_path.to_owned(),
                // The hook-driven activate path knows nothing of Delta's
                // launch context: the spawn-time snapshot is recorded once by
                // `insert_spawning_session` and is left untouched here. For
                // an externally-started `claude` (the fresh-insert side of
                // `register_session`) Delta likewise has no launch git
                // context, so all three stay `None`.
                branch_at_launch: None,
                repo_root: None,
                repository_display_name: None,
            })
            .await?;
        events.push(SessionEvent::SessionRegistered {
            session_id: session_id.clone(),
        });
        Ok((session, main_id))
    }
}

/// Require `transcript_path` to be a `.jsonl` file resolving under `root`.
///
/// The transcript is read from disk and its parsed lines are surfaced to the
/// browser, so an unconfined path is a file-disclosure primitive: a forged (or
/// the first, previously-unvalidated) hook could name any readable file. Two
/// checks close that:
///
/// - the path must end in `.jsonl` (real Claude Code transcripts always do), and
/// - it must resolve *under* `root` once `..` and symlinks are collapsed.
///
/// To defeat `..` without falsely rejecting a legitimate transcript that has not
/// been created yet, the *parent directory* is canonicalized (it exists —
/// Claude Code creates the per-project directory before writing into it) and the
/// file name is re-joined onto the real parent, rather than canonicalizing the
/// (possibly missing) file itself. `root` is canonicalized too, so a symlinked
/// root (e.g. macOS's `/tmp` → `/private/tmp`) still matches. `Path::starts_with`
/// compares whole path components, so a sibling like `<root>x` cannot slip past a
/// string-prefix match.
fn validate_transcript_path(root: &str, transcript_path: &str) -> Result<()> {
    let reject = |reason: &str| {
        Err(Error::InvalidTranscriptPath(format!(
            "{transcript_path}: {reason}"
        )))
    };

    let path = Path::new(transcript_path);
    if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
        return reject("must be a .jsonl file");
    }

    let canonical_root = match std::fs::canonicalize(root) {
        Ok(root) => root,
        Err(err) => {
            return reject(&format!("transcript root {root} is unresolvable: {err}"));
        }
    };

    let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) else {
        return reject("has no parent directory");
    };
    // An empty parent means the path was a bare relative file name; canonicalize
    // resolves that against the process cwd, which is never the transcript root,
    // so it is correctly refused below.
    let canonical_parent = match std::fs::canonicalize(parent) {
        Ok(parent) => parent,
        Err(err) => {
            return reject(&format!("parent directory is unresolvable: {err}"));
        }
    };
    let resolved = canonical_parent.join(file_name);
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
    /// exist yet — the parent directory is what has to resolve.
    #[test]
    fn accepts_a_jsonl_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let path = dir.path().join("not-created-yet.jsonl");
        assert!(validate_transcript_path(root, path.to_str().unwrap()).is_ok());
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
