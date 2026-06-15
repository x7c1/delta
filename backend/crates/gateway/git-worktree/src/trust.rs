//! Seeding Claude Code's per-directory workspace-trust flag.
//!
//! Claude Code records workspace trust per absolute path in the user's global
//! config (`~/.claude.json`) under
//! `projects.<abs-path>.hasTrustDialogAccepted = true`. Launching `claude`
//! interactively in a fresh directory that contains files pops a blocking
//! "Do you trust the files in this folder?" dialog; there is no CLI flag to
//! accept it. Pre-seeding that key before launch is the only way to keep a
//! programmatic launch (e.g. in a freshly-created git worktree) from stalling
//! on the dialog.
//!
//! ## Concurrency
//!
//! delta-server is a single process, so a [`tokio::sync::Mutex`] held across the
//! whole read-modify-write serializes Delta's own writes. The `claude` binary is
//! an uncoordinated writer of the same file; that residual delta-vs-claude race
//! is accepted (we cannot lock against another process here and deliberately do
//! not introduce file locking). The write is atomic (temp file + rename), so a
//! reader never sees a partially-written file.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::Error;

/// The config key Claude Code reads to skip the workspace-trust dialog.
const TRUST_KEY: &str = "hasTrustDialogAccepted";

/// The top-level object keyed by absolute project path.
const PROJECTS_KEY: &str = "projects";

/// Ensure `dir` is marked trusted in the config file at `config_path`.
///
/// Idempotent: if the key is already `true`, the file is left untouched (no
/// rewrite). A missing config file starts from an empty object; a config file
/// that does not parse as JSON yields an error and is left untouched, so a
/// corrupt or hand-edited file is never clobbered.
pub(crate) async fn ensure_dir_trusted(config_path: &Path, dir: &str) -> Result<(), Error> {
    // Read the existing config, if any. A missing file is the normal first-run
    // case and starts from an empty object; any other read error propagates.
    let mut root = match tokio::fs::read(config_path).await {
        Ok(bytes) => parse_config(config_path, &bytes)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Value::Object(Default::default()),
        Err(source) => return Err(io_error(config_path, source)),
    };

    // Walk to `projects.<dir>`, creating the intermediate objects as needed.
    // `as_object_mut` returns `None` for a non-object value already at the key
    // (e.g. a user-edited `projects: "x"`); replace it with an object so we
    // never panic, mirroring how Claude Code itself would re-establish the
    // shape rather than failing.
    let projects = root
        .as_object_mut()
        .expect("config root is constructed/parsed as a JSON object")
        .entry(PROJECTS_KEY)
        .or_insert_with(|| Value::Object(Default::default()));
    if !projects.is_object() {
        *projects = Value::Object(Default::default());
    }
    let project = projects
        .as_object_mut()
        .expect("projects is an object")
        .entry(dir)
        .or_insert_with(|| Value::Object(Default::default()));
    if !project.is_object() {
        *project = Value::Object(Default::default());
    }
    let project = project.as_object_mut().expect("project entry is an object");

    // Idempotent fast path: already trusted, so skip the write entirely. This
    // minimizes churn and shrinks the read-modify-write window for the common
    // case where the dir was seeded on an earlier spawn/resume.
    if project.get(TRUST_KEY) == Some(&Value::Bool(true)) {
        return Ok(());
    }
    project.insert(TRUST_KEY.to_owned(), Value::Bool(true));

    let serialized = serde_json::to_vec_pretty(&root).map_err(|source| Error::TrustSerialize {
        path: config_path.display().to_string(),
        source,
    })?;
    write_atomic(config_path, dir, &serialized).await
}

/// Parse the existing config bytes, mapping a parse failure to [`Error::TrustParse`]
/// (the file is left untouched by the caller, never overwritten).
fn parse_config(config_path: &Path, bytes: &[u8]) -> Result<Value, Error> {
    serde_json::from_slice(bytes).map_err(|source| Error::TrustParse {
        path: config_path.display().to_string(),
        source,
    })
}

/// Write `bytes` to `config_path` atomically: serialize to a temp file in the
/// same directory, then `rename` over the target (atomic on the same
/// filesystem). The temp file is removed on a write/rename error so a failure
/// leaves no stray file behind.
async fn write_atomic(config_path: &Path, dir: &str, bytes: &[u8]) -> Result<(), Error> {
    let tmp = temp_path(config_path, dir);
    if let Err(source) = tokio::fs::write(&tmp, bytes).await {
        // Best-effort cleanup; report the original write error.
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(io_error(&tmp, source));
    }
    if let Err(source) = tokio::fs::rename(&tmp, config_path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(io_error(config_path, source));
    }
    Ok(())
}

/// A temp-file path in the same directory as the config, suffixed with a nonce
/// derived from `dir` (not a clock or RNG) so concurrent seeds of *different*
/// dirs do not collide on the temp name while staying deterministic.
fn temp_path(config_path: &Path, dir: &str) -> PathBuf {
    let nonce = djb2(dir);
    let mut name = config_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".claude.json".to_owned());
    name.push_str(&format!(".delta-{nonce:x}.tmp"));
    match config_path.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

/// A tiny non-cryptographic hash (djb2) of `s`, used only to derive a stable
/// per-dir temp-file nonce. Avoids pulling in time/RNG just to name a temp file.
fn djb2(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for b in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(b));
    }
    hash
}

/// Build an [`Error::TrustIo`] carrying the offending path.
fn io_error(path: &Path, source: std::io::Error) -> Error {
    Error::TrustIo {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read the config file back as a parsed JSON value.
    async fn read_json(path: &Path) -> Value {
        let bytes = tokio::fs::read(path).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn trusted(value: &Value, dir: &str) -> bool {
        value
            .get(PROJECTS_KEY)
            .and_then(|p| p.get(dir))
            .and_then(|d| d.get(TRUST_KEY))
            == Some(&Value::Bool(true))
    }

    #[tokio::test]
    async fn missing_file_creates_trusted_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join(".claude.json");
        let dir = "/home/u/repos/project";

        ensure_dir_trusted(&config, dir).await.unwrap();

        let value = read_json(&config).await;
        assert!(trusted(&value, dir), "the dir is trusted, got {value}");
        // Nothing else leaked into the file.
        assert_eq!(
            value.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec![PROJECTS_KEY]
        );
    }

    #[tokio::test]
    async fn preserves_other_keys_and_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join(".claude.json");
        let dir = "/home/u/repos/new";
        let initial = serde_json::json!({
            "numStartups": 7,
            "oauthAccount": { "emailAddress": "u@example.com" },
            "projects": {
                "/home/u/repos/existing": {
                    "hasTrustDialogAccepted": true,
                    "history": ["a", "b"]
                }
            }
        });
        tokio::fs::write(&config, serde_json::to_vec_pretty(&initial).unwrap())
            .await
            .unwrap();

        ensure_dir_trusted(&config, dir).await.unwrap();

        let value = read_json(&config).await;
        // The new dir is trusted.
        assert!(trusted(&value, dir));
        // Every pre-existing top-level key is preserved verbatim.
        assert_eq!(value.get("numStartups"), Some(&Value::from(7)));
        assert_eq!(
            value.get("oauthAccount"),
            initial.get("oauthAccount"),
            "unrelated top-level objects are preserved"
        );
        // The other project entry (and its non-trust keys) is untouched.
        let existing = value
            .get(PROJECTS_KEY)
            .and_then(|p| p.get("/home/u/repos/existing"))
            .unwrap();
        assert_eq!(existing.get("history"), Some(&serde_json::json!(["a", "b"])));
        assert!(trusted(&value, "/home/u/repos/existing"));
    }

    #[tokio::test]
    async fn already_trusted_is_idempotent_no_rewrite() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join(".claude.json");
        let dir = "/home/u/repos/project";
        ensure_dir_trusted(&config, dir).await.unwrap();

        // Capture mtime + bytes, then re-run: an idempotent no-op must not
        // rewrite the file.
        let before_bytes = tokio::fs::read(&config).await.unwrap();
        let before_mtime = tokio::fs::metadata(&config).await.unwrap().modified().unwrap();

        ensure_dir_trusted(&config, dir).await.unwrap();

        let after_bytes = tokio::fs::read(&config).await.unwrap();
        let after_mtime = tokio::fs::metadata(&config).await.unwrap().modified().unwrap();
        assert_eq!(before_bytes, after_bytes, "content unchanged");
        assert_eq!(before_mtime, after_mtime, "file was not rewritten");
        assert!(trusted(&read_json(&config).await, dir));
    }

    #[tokio::test]
    async fn corrupt_file_errors_and_is_left_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join(".claude.json");
        let garbage = b"{ this is not json ";
        tokio::fs::write(&config, garbage).await.unwrap();

        let err = ensure_dir_trusted(&config, "/home/u/repos/project")
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::TrustParse { .. }),
            "a non-JSON config yields a parse error, got {err:?}"
        );
        // The corrupt file is left exactly as it was — never clobbered.
        assert_eq!(tokio::fs::read(&config).await.unwrap(), garbage);
    }
}
