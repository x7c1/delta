//! Owner-only directory creation, never through a symlink: a session's scratch
//! working directory, and the directory holding the settings file.

use std::io::ErrorKind;
use std::path::Path;

use super::FsWorkspace;
use crate::error::Error;

/// Permission bits for the directories this module creates: owner-only, so
/// nothing in them can be swapped out from under Delta by another local user.
pub(super) const PRIVATE_DIR_MODE: u32 = 0o700;

impl FsWorkspace {
    /// Create the directory `path` owner-only, as
    /// [`delta_usecase::Workspace::create_private_dir`] describes, refusing one
    /// that exists as a symlink or as anything but a directory.
    pub(super) async fn create_dir(&self, path: &str) -> Result<(), Error> {
        let dir = Path::new(path);
        create_private_dir_all(dir).await?;
        if !tokio::fs::metadata(dir).await?.is_dir() {
            return Err(Error::UnsafePath(format!(
                "{}: exists and is not a directory",
                dir.display()
            )));
        }
        Ok(())
    }
}

/// Create `dir` (and any missing ancestors) with owner-only permissions,
/// refusing a path that already exists as a symlink.
///
/// `DirBuilder::mode` is what makes the new directories 0700: a bare
/// `create_dir_all` asks for 0777 and lets the process umask decide, which on a
/// typical host lands at 0755 — group- and world-readable. The explicit mode is
/// applied by `mkdir(2)` itself and is not subject to the umask.
///
/// An *existing* directory is left exactly as it is (its mode included): it
/// may be one Delta does not own (a data directory pointed somewhere shared).
/// The symlink refusal is the guard that matters there — hardening only the
/// file would leave the directory as the swap target, so a pre-planted
/// `…/settings -> /somewhere/else` link must fail the write rather than
/// redirect it.
pub(super) async fn create_private_dir_all(dir: &Path) -> Result<(), Error> {
    match tokio::fs::symlink_metadata(dir).await {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(Error::UnsafePath(format!(
                "{}: the directory is a symlink",
                dir.display()
            )));
        }
        // Already a real directory (or a file, which each caller rejects:
        // `create_dir` checks for a directory, the settings write cannot open a
        // file under it): nothing to create.
        Ok(_) => return Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    tokio::fs::DirBuilder::new()
        .recursive(true)
        .mode(PRIVATE_DIR_MODE)
        .create(dir)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use delta_usecase::Workspace;

    use super::*;

    #[tokio::test]
    async fn creates_a_private_dir_owner_only_with_its_parents() {
        let dir = tempfile::tempdir().unwrap();
        // The scratch directory of a session: its parent (`sessions/`) is
        // normally there already, but a missing one is created too.
        let scratch = dir.path().join("sessions").join("delta-1");

        let ws = FsWorkspace::new();
        ws.create_private_dir(scratch.to_str().unwrap())
            .await
            .unwrap();
        // Idempotent: a directory that already exists is accepted as it is.
        ws.create_private_dir(scratch.to_str().unwrap())
            .await
            .unwrap();

        let mode = tokio::fs::metadata(&scratch)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            PRIVATE_DIR_MODE,
            "a session's working directory is its owner's alone"
        );
    }

    #[tokio::test]
    async fn refuses_to_create_a_private_dir_through_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        tokio::fs::create_dir(&elsewhere).await.unwrap();
        let scratch = dir.path().join("delta-1");
        std::os::unix::fs::symlink(&elsewhere, &scratch).unwrap();

        let err = FsWorkspace::new()
            .create_private_dir(scratch.to_str().unwrap())
            .await
            .unwrap_err();

        assert!(
            matches!(err, delta_usecase::Error::Workspace(_)),
            "a symlinked directory is a workspace failure, got {err:?}"
        );
    }

    #[tokio::test]
    async fn refuses_a_private_dir_that_is_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = dir.path().join("delta-1");
        tokio::fs::write(&scratch, "").await.unwrap();

        let err = FsWorkspace::new()
            .create_private_dir(scratch.to_str().unwrap())
            .await
            .unwrap_err();

        assert!(
            matches!(err, delta_usecase::Error::Workspace(_)),
            "a file where the directory goes is a workspace failure, got {err:?}"
        );
    }
}
