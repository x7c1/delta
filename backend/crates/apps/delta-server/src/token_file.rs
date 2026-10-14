//! The bearer token file: how a local client of the same user finds the token
//! of the `delta-server` running on a data directory.
//!
//! The server mints its token at startup and holds it in memory; a page gets it
//! through a one-time code (`crate::one_time_codes`). A non-browser client —
//! `delta-server open-url`, which asks the running server for a fresh code, or
//! a script — needs the token itself, so the `delta-server` binary writes it to
//! [`DataLayout::auth_token`](delta_bootstrap::DataLayout::auth_token) on start
//! and removes it on a clean stop.
//!
//! The file is created owner-only (`0600`), inside a data directory that is
//! owner-only too (`0700`), so only the user who runs the server can read it.
//! A file left by an earlier run is replaced, never reused: it is removed and
//! the new one created exclusively, so its mode is always the one set here.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

/// The token file's mode: read and write for the owner only.
pub const TOKEN_FILE_MODE: u32 = 0o600;

/// The token file of a running server. Dropping it removes the file.
#[derive(Debug)]
pub struct TokenFile {
    path: PathBuf,
}

impl TokenFile {
    /// Write `token` to `path`, owner-only, replacing any file already there.
    pub fn write(path: impl Into<PathBuf>, token: &str) -> std::io::Result<Self> {
        let path = path.into();
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(TOKEN_FILE_MODE)
            .open(&path)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
        Ok(Self { path })
    }

    /// Where the token is kept.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TokenFile {
    /// Remove the file. It is already gone after an erase, which deletes the
    /// whole data directory; any other failure is logged, since the next start
    /// replaces the file anyway.
    fn drop(&mut self) {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(
                path = %self.path.display(),
                "could not remove the bearer token file: {err}"
            ),
        }
    }
}

/// Read the token a running server keeps at `path`, without the trailing
/// newline an editor may have added.
pub fn read_token(path: &Path) -> std::io::Result<String> {
    let token = std::fs::read_to_string(path)?;
    Ok(token.trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    #[test]
    fn the_file_is_owner_only_and_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth-token");

        let file = TokenFile::write(&path, "the-token").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, TOKEN_FILE_MODE);
        assert_eq!(read_token(&path).unwrap(), "the-token");

        drop(file);
        assert!(!path.exists(), "a clean stop removes the file");
    }

    #[test]
    fn a_stale_file_is_replaced_with_an_owner_only_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth-token");
        std::fs::write(&path, "stale").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let _file = TokenFile::write(&path, "fresh").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, TOKEN_FILE_MODE);
        assert_eq!(read_token(&path).unwrap(), "fresh");
    }

    #[test]
    fn a_file_already_gone_is_not_an_error_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth-token");
        let file = TokenFile::write(&path, "t").unwrap();
        std::fs::remove_file(&path).unwrap();
        drop(file);
    }
}
