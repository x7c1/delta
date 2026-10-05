//! The settings-file write: an owner-only file under an owner-only directory,
//! never reached through a symlink.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tokio::io::AsyncWriteExt;

use super::create_dir::create_private_dir_all;
use super::FsWorkspace;
use crate::error::Error;

/// Permission bits for the settings file: owner read/write, nobody else.
const SETTINGS_FILE_MODE: u32 = 0o600;

impl FsWorkspace {
    /// Write the session settings JSON, owner-readable only.
    ///
    /// The settings file is `settings/<port>.json` in the server's data
    /// directory, and it is doubly sensitive: it embeds the hook secret in
    /// every hook URL, and its `statusLine` / `SessionStart` entries are
    /// commands Claude Code executes. The data directory is created owner-only,
    /// but it can be pointed anywhere (`DELTA_DATA_DIR`), so the write does not
    /// rely on it: another local user must not be able to read the secret out
    /// of a 0644 file, or pre-plant a symlink (for the file or its directory)
    /// and have Delta write commands into a file of their choosing. Hence: an
    /// owner-only parent directory, an owner-only file, and a refusal to follow
    /// a symlink at either level.
    ///
    /// Overwrite semantics are deliberate (see `overwrites_existing_settings`):
    /// the file is truncated on every write so the hook URLs stay current.
    pub(super) async fn write(
        &self,
        settings_path: &str,
        settings_json: &str,
    ) -> Result<(), Error> {
        let path = Path::new(settings_path);
        if let Some(parent) = path.parent() {
            create_private_dir_all(parent).await?;
        }
        write_private_file(path, settings_json.as_bytes()).await
    }
}

/// Write `bytes` to `path` as an owner-only file, never following a symlink.
///
/// `O_NOFOLLOW` makes the `open(2)` itself fail when the final component is a
/// symlink, so a pre-planted link is refused *before* any byte is written —
/// unlike `fs::write`, which happily follows one. `mode` covers the file Delta
/// creates; the explicit `set_permissions` afterwards also tightens a file left
/// behind at 0644 by an older Delta run (`mode` applies only on creation).
async fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(SETTINGS_FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .await?;
    file.set_permissions(std::fs::Permissions::from_mode(SETTINGS_FILE_MODE))
        .await?;
    file.write_all(bytes).await?;
    file.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use delta_usecase::Workspace;

    use super::super::create_dir::PRIVATE_DIR_MODE;
    use super::*;

    #[tokio::test]
    async fn writes_settings_creating_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        // A nested path whose parents do not exist yet: the write must create them.
        let settings_path = dir.path().join("run").join("settings.json");
        let settings_path_str = settings_path.to_str().unwrap();

        let ws = FsWorkspace::new();
        ws.write_session_settings(settings_path_str, r#"{"hooks":{}}"#)
            .await
            .unwrap();

        let written = tokio::fs::read_to_string(&settings_path).await.unwrap();
        assert_eq!(written, r#"{"hooks":{}}"#);
    }

    #[tokio::test]
    async fn overwrites_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        let settings_path_str = settings_path.to_str().unwrap();
        let ws = FsWorkspace::new();

        ws.write_session_settings(settings_path_str, "old")
            .await
            .unwrap();
        ws.write_session_settings(settings_path_str, "new")
            .await
            .unwrap();

        let written = tokio::fs::read_to_string(&settings_path).await.unwrap();
        assert_eq!(
            written, "new",
            "settings are rewritten so hook URLs stay current"
        );
    }

    #[tokio::test]
    async fn writes_settings_with_owner_only_permissions() {
        let dir = tempfile::tempdir().unwrap();
        // The parent is created by the write too, so both modes are asserted:
        // the file must be 0600 and the directory Delta made 0700, or the hook
        // secret embedded in the settings is readable by every local user on a
        // shared host.
        let parent = dir.path().join("settings");
        let settings_path = parent.join("4000.json");

        let ws = FsWorkspace::new();
        ws.write_session_settings(settings_path.to_str().unwrap(), r#"{"hooks":{}}"#)
            .await
            .unwrap();

        let file_mode = tokio::fs::metadata(&settings_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            file_mode & 0o777,
            SETTINGS_FILE_MODE,
            "the settings file carries the hook secret, so only its owner may read it"
        );
        let dir_mode = tokio::fs::metadata(&parent)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            dir_mode & 0o777,
            PRIVATE_DIR_MODE,
            "a group/world-writable parent would let another user swap the file"
        );
    }

    #[tokio::test]
    async fn rewrites_a_leftover_settings_file_as_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        // A file left behind by an older Delta run at the default 0644: the
        // creation mode does not apply to it, so the write must tighten it.
        tokio::fs::write(&settings_path, "old").await.unwrap();
        tokio::fs::set_permissions(&settings_path, std::fs::Permissions::from_mode(0o644))
            .await
            .unwrap();

        let ws = FsWorkspace::new();
        ws.write_session_settings(settings_path.to_str().unwrap(), "new")
            .await
            .unwrap();

        let mode = tokio::fs::metadata(&settings_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, SETTINGS_FILE_MODE);
    }

    #[tokio::test]
    async fn refuses_to_write_settings_through_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        // A pre-planted symlink standing where the settings file goes: Delta
        // must refuse rather than write the hook secret (and the commands Claude
        // Code runs) into whatever it points at.
        let target = dir.path().join("attacker-owned.json");
        tokio::fs::write(&target, "untouched").await.unwrap();
        let settings_path = dir.path().join("settings.json");
        std::os::unix::fs::symlink(&target, &settings_path).unwrap();

        let ws = FsWorkspace::new();
        let err = ws
            .write_session_settings(settings_path.to_str().unwrap(), "secret")
            .await
            .unwrap_err();

        assert!(
            matches!(err, delta_usecase::Error::Workspace(_)),
            "a symlinked settings path is a workspace failure, got {err:?}"
        );
        assert_eq!(
            tokio::fs::read_to_string(&target).await.unwrap(),
            "untouched",
            "the link target must not be written or truncated"
        );
    }

    #[tokio::test]
    async fn refuses_to_write_settings_through_a_symlinked_parent() {
        let dir = tempfile::tempdir().unwrap();
        // Hardening the file alone would leave the directory as the swap
        // target, so a symlinked parent is refused before anything is written.
        let elsewhere = dir.path().join("elsewhere");
        tokio::fs::create_dir(&elsewhere).await.unwrap();
        let parent = dir.path().join("settings");
        std::os::unix::fs::symlink(&elsewhere, &parent).unwrap();

        let ws = FsWorkspace::new();
        let err = ws
            .write_session_settings(parent.join("4000.json").to_str().unwrap(), "secret")
            .await
            .unwrap_err();

        assert!(
            matches!(err, delta_usecase::Error::Workspace(_)),
            "a symlinked settings directory is a workspace failure, got {err:?}"
        );
        assert!(
            !elsewhere.join("4000.json").exists(),
            "nothing is written through the redirected directory"
        );
    }
}
