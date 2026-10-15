//! Owner-only directory creation, never through a symlink: the directory
//! holding the settings file.

use std::io::ErrorKind;
use std::path::Path;

use crate::error::Error;

/// Permission bits for the directories this module creates: owner-only, so
/// nothing in them can be swapped out from under Delta by another local user.
pub(super) const PRIVATE_DIR_MODE: u32 = 0o700;

/// Create `dir` (and any missing ancestors) with owner-only permissions,
/// refusing a path that already exists as a symlink.
///
/// `DirBuilder::mode` is what makes the new directories 0700: a bare
/// `create_dir_all` asks for 0777 and lets the process umask decide, which on a
/// typical host lands at 0755 — group- and world-readable. The explicit mode is
/// still filtered by the umask (`mkdir(2)` creates `mode & ~umask`), but a
/// umask only ever removes bits, so the result is never wider than 0700, and
/// is exactly 0700 under any umask that leaves the owner's bits alone.
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
        // Already a real directory (or a file, which the caller rejects: the
        // settings write cannot open a file under it): nothing to create.
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
