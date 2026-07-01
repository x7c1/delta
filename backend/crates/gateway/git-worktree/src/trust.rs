//! Pre-accepting Claude Code's blocking startup dialogs for a directory.
//!
//! Claude Code shows blocking interactive prompts at startup whose state lives
//! per absolute path in the user's global config (`~/.claude.json`) under
//! `projects.<abs-path>.<key>`. None of these prompts have a CLI flag to
//! accept; pre-seeding the keys before launch is the only way to keep a
//! programmatic launch (e.g. in a freshly-created git worktree) from stalling.
//!
//! Two distinct prompts are pre-accepted here:
//!
//! - **Workspace trust** — "Do you trust the files in this folder?". Fires
//!   when `claude` is launched in a directory it has never seen before.
//!   Skipped by setting `hasTrustDialogAccepted = true`.
//! - **External CLAUDE.md imports** — "Allow external CLAUDE.md file
//!   imports?". Fires at startup when an ancestor `CLAUDE.md` file uses
//!   `@`-import syntax that points to paths outside the launch directory
//!   (i.e. external from the launch dir's viewpoint). Skipped by setting
//!   both `hasClaudeMdExternalIncludesApproved = true` and
//!   `hasClaudeMdExternalIncludesWarningShown = true` — Claude Code re-shows
//!   the prompt unless both flags are present.
//!
//! Both prompts are blocking and non-interactive launches (Delta's spawn flow)
//! cannot answer them, so the reaper would eventually kill the spawn as
//! `SpawnFailed`. Pre-seeding both is what keeps fresh-workdir spawns alive.
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

/// The config key Claude Code reads to skip the "Allow external CLAUDE.md file
/// imports?" prompt. Must be paired with [`EXTERNAL_INCLUDES_WARNING_SHOWN_KEY`]:
/// Claude Code re-shows the prompt unless both keys are set.
const EXTERNAL_INCLUDES_APPROVED_KEY: &str = "hasClaudeMdExternalIncludesApproved";

/// The companion flag indicating Claude Code has already shown its external-
/// includes warning for this directory; setting it alongside the approval key
/// is what fully suppresses the prompt on the next launch.
const EXTERNAL_INCLUDES_WARNING_SHOWN_KEY: &str = "hasClaudeMdExternalIncludesWarningShown";

/// All boolean keys this module pre-seeds to `true` on a project entry. Kept
/// in one place so the seeding loop and the idempotency check stay in sync.
const SEEDED_KEYS: &[&str] = &[
    TRUST_KEY,
    EXTERNAL_INCLUDES_APPROVED_KEY,
    EXTERNAL_INCLUDES_WARNING_SHOWN_KEY,
];

/// The top-level object keyed by absolute project path.
const PROJECTS_KEY: &str = "projects";

/// Ensure `dir` is pre-accepted for every blocking startup dialog this module
/// covers (workspace trust and external CLAUDE.md imports) in the config file
/// at `config_path`.
///
/// Idempotent: if every seeded key is already `true`, the file is left
/// untouched (no rewrite). A missing config file starts from an empty object;
/// a config file that does not parse as JSON yields an error and is left
/// untouched, so a corrupt or hand-edited file is never clobbered.
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

    // Idempotent fast path: if every seeded key is already `true`, skip the
    // write entirely. This minimizes churn and shrinks the read-modify-write
    // window for the common case where the dir was seeded on an earlier
    // spawn/resume. A partial seed (e.g. only `hasTrustDialogAccepted` from an
    // older Delta) still triggers a rewrite to fill in the missing keys.
    let mut changed = false;
    for &key in SEEDED_KEYS {
        if project.get(key) != Some(&Value::Bool(true)) {
            project.insert(key.to_owned(), Value::Bool(true));
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }

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

    /// True iff every key this module seeds is `true` on `projects.<dir>`.
    fn fully_seeded(value: &Value, dir: &str) -> bool {
        let Some(project) = value.get(PROJECTS_KEY).and_then(|p| p.get(dir)) else {
            return false;
        };
        SEEDED_KEYS
            .iter()
            .all(|key| project.get(*key) == Some(&Value::Bool(true)))
    }

    #[tokio::test]
    async fn missing_file_creates_trusted_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join(".claude.json");
        let dir = "/home/u/repos/project";

        ensure_dir_trusted(&config, dir).await.unwrap();

        let value = read_json(&config).await;
        assert!(
            fully_seeded(&value, dir),
            "all three seeded keys are true, got {value}"
        );
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
        // The new dir is fully seeded.
        assert!(fully_seeded(&value, dir));
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
        assert_eq!(
            existing.get("history"),
            Some(&serde_json::json!(["a", "b"]))
        );
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
        let before_mtime = tokio::fs::metadata(&config)
            .await
            .unwrap()
            .modified()
            .unwrap();

        ensure_dir_trusted(&config, dir).await.unwrap();

        let after_bytes = tokio::fs::read(&config).await.unwrap();
        let after_mtime = tokio::fs::metadata(&config)
            .await
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before_bytes, after_bytes, "content unchanged");
        assert_eq!(before_mtime, after_mtime, "file was not rewritten");
        assert!(fully_seeded(&read_json(&config).await, dir));
    }

    #[tokio::test]
    async fn fully_seeded_is_idempotent_when_all_keys_already_true() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join(".claude.json");
        let dir = "/home/u/repos/project";
        let initial = serde_json::json!({
            PROJECTS_KEY: {
                dir: {
                    TRUST_KEY: true,
                    EXTERNAL_INCLUDES_APPROVED_KEY: true,
                    EXTERNAL_INCLUDES_WARNING_SHOWN_KEY: true,
                }
            }
        });
        tokio::fs::write(&config, serde_json::to_vec_pretty(&initial).unwrap())
            .await
            .unwrap();

        ensure_dir_trusted(&config, dir).await.unwrap();

        // No-op: the JSON shape is identical to what we wrote.
        let value = read_json(&config).await;
        assert_eq!(value, initial, "the file is unchanged, got {value}");
    }

    #[tokio::test]
    async fn partial_seed_fills_in_missing_external_includes_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join(".claude.json");
        let dir = "/home/u/repos/project";
        // An older Delta (or hand-edit) wrote only the trust key; the two
        // external-includes keys are missing.
        let initial = serde_json::json!({
            PROJECTS_KEY: {
                dir: {
                    TRUST_KEY: true,
                    "history": ["a", "b"],
                }
            }
        });
        tokio::fs::write(&config, serde_json::to_vec_pretty(&initial).unwrap())
            .await
            .unwrap();

        ensure_dir_trusted(&config, dir).await.unwrap();

        let value = read_json(&config).await;
        // The two missing keys were added.
        assert!(
            fully_seeded(&value, dir),
            "all three seeded keys are true, got {value}"
        );
        // The pre-existing key is still true (left alone, not rewritten away).
        assert!(trusted(&value, dir));
        // Unrelated keys on the same project entry are preserved verbatim.
        let project = value.get(PROJECTS_KEY).and_then(|p| p.get(dir)).unwrap();
        assert_eq!(project.get("history"), Some(&serde_json::json!(["a", "b"])));
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
