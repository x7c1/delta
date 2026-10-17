use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use anyhow::{bail, Context};

/// Make sure `dir` is a directory no other user can write into: owned by this
/// process's effective user, with no write permission for group or others. A
/// directory others could write into would let them take the socket's path
/// first, which is what the lock is there to prevent.
pub fn ensure_private_dir(dir: &Path) -> anyhow::Result<()> {
    let metadata =
        std::fs::metadata(dir).with_context(|| format!("could not inspect {}", dir.display()))?;
    if !metadata.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    let euid = unsafe { libc::geteuid() };
    if metadata.uid() != euid {
        bail!(
            "{} is owned by uid {}, not by this user ({euid})",
            dir.display(),
            metadata.uid()
        );
    }
    let mode = metadata.permissions().mode() & 0o7777;
    if mode & 0o022 != 0 {
        bail!(
            "{} is writable by other users (mode {mode:o})",
            dir.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn an_owner_only_directory_is_private() {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        ensure_private_dir(dir.path()).unwrap();
    }

    #[test]
    fn a_directory_others_can_write_into_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        for mode in [0o777, 0o1777, 0o720, 0o702] {
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(mode)).unwrap();
            assert!(
                ensure_private_dir(dir.path()).is_err(),
                "mode {mode:o} was taken as private"
            );
        }
    }

    #[test]
    fn a_missing_directory_or_a_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ensure_private_dir(&dir.path().join("missing")).is_err());
        let file = dir.path().join("file");
        fs::write(&file, "").unwrap();
        assert!(ensure_private_dir(&file).is_err());
    }
}
