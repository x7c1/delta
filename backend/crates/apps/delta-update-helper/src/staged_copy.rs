//! Copying the update file somewhere only root can change it.

use std::fs::{File, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::Refusal;

/// The largest file the helper copies. A release's `.deb` is a few tens of
/// MB; anything far beyond is not one, and copying it would only fill the
/// disk.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// What the copy is called inside its directory.
const COPY_NAME: &str = "update.deb";

/// The update file, copied into a directory only root may enter, with the
/// sha256 of what was copied.
///
/// The original lives in the user's data directory, which the user (and
/// anything running as the user) can change at any time; the copy cannot be
/// swapped between the checks and `apt-get` reading it, since everything the
/// helper checks and installs is the copy. Dropping it removes the directory.
pub struct StagedCopy {
    dir: TempDir,
    sha256: String,
}

impl StagedCopy {
    /// Copy `source` into a new directory under `parent`, created mode 0700.
    ///
    /// Refused unless `source` is a regular file of at most
    /// [`MAX_FILE_BYTES`]: a symlink is refused (it is opened with
    /// `O_NOFOLLOW`, so one swapped in after a check is refused too), and so
    /// is a directory, a device or a pipe (opened with `O_NONBLOCK`, so a
    /// pipe cannot hold the helper up). Size and type are read from the open
    /// file, not the path.
    pub fn copy(source: &Path, parent: &Path) -> Result<Self, Refusal> {
        let refuse = |reason: String| Refusal::File {
            path: source.to_path_buf(),
            reason,
        };
        let unreadable = |err| Refusal::FileUnreadable {
            path: source.to_path_buf(),
            source: err,
        };
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(source)
            .map_err(|err| match err.raw_os_error() {
                Some(libc::ELOOP) => refuse("it is a symlink".to_owned()),
                _ => unreadable(err),
            })?;
        let metadata = file.metadata().map_err(unreadable)?;
        if !metadata.is_file() {
            return Err(refuse("it is not a regular file".to_owned()));
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(refuse(format!(
                "it is {} bytes, more than the {MAX_FILE_BYTES} an update may be",
                metadata.len()
            )));
        }

        let dir = tempfile::Builder::new()
            .prefix("delta-update-")
            .permissions(Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .map_err(|source| Refusal::Io {
                what: format!("could not create a directory in {}", parent.display()),
                source,
            })?;
        let copy_path = dir.path().join(COPY_NAME);
        let io = |source| Refusal::Io {
            what: format!("could not copy the update file to {}", copy_path.display()),
            source,
        };
        let mut copy = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&copy_path)
            .map_err(io)?;
        let sha256 =
            copy_hashing(&mut file, &mut copy, MAX_FILE_BYTES).map_err(|err| match err {
                CopyError::Read(err) => unreadable(err),
                CopyError::Write(err) => io(err),
                CopyError::TooLarge => refuse(format!(
                    "it grew past the {MAX_FILE_BYTES} bytes an update may be while it was copied"
                )),
            })?;
        copy.sync_all().map_err(io)?;
        Ok(Self { dir, sha256 })
    }

    /// The copy.
    pub fn path(&self) -> PathBuf {
        self.dir.path().join(COPY_NAME)
    }

    /// The copy's sha256, lowercase hex.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

enum CopyError {
    Read(std::io::Error),
    Write(std::io::Error),
    TooLarge,
}

/// Copy `from` into `to`, at most `limit` bytes, and return the sha256 of
/// what was copied as lowercase hex.
fn copy_hashing(from: &mut File, to: &mut File, limit: u64) -> Result<String, CopyError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut copied = 0u64;
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(CopyError::Read(err)),
        };
        copied += read as u64;
        if copied > limit {
            return Err(CopyError::TooLarge);
        }
        hasher.update(&buffer[..read]);
        to.write_all(&buffer[..read]).map_err(CopyError::Write)?;
    }
    Ok(hex(&hasher.finalize()))
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    /// sha256 of `hello\n`.
    const HELLO_SHA256: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

    #[test]
    fn a_regular_file_is_copied_into_an_owner_only_directory() {
        let source_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("delta-desktop_0.6.0_amd64.deb");
        std::fs::write(&source, "hello\n").unwrap();

        let staged = StagedCopy::copy(&source, parent.path()).unwrap();
        assert_eq!(staged.sha256(), HELLO_SHA256);
        assert_eq!(std::fs::read(staged.path()).unwrap(), b"hello\n");
        let dir = staged.path().parent().unwrap().to_path_buf();
        assert_eq!(dir.parent().unwrap(), parent.path());
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        // Changing the original afterwards does not change the copy.
        std::fs::write(&source, "swapped\n").unwrap();
        assert_eq!(std::fs::read(staged.path()).unwrap(), b"hello\n");

        drop(staged);
        assert!(!dir.exists(), "dropping the copy removes its directory");
    }

    #[test]
    fn a_symlink_is_refused() {
        let source_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let target = source_dir.path().join("real.deb");
        std::fs::write(&target, "hello\n").unwrap();
        let link = source_dir.path().join("delta-desktop_0.6.0_amd64.deb");
        symlink(&target, &link).unwrap();

        let err = StagedCopy::copy(&link, parent.path()).err().unwrap();
        assert!(
            matches!(&err, Refusal::File { reason, .. } if reason == "it is a symlink"),
            "{err:?}"
        );
        assert_eq!(
            std::fs::read_dir(parent.path()).unwrap().count(),
            0,
            "nothing is staged"
        );
    }

    #[test]
    fn a_directory_a_pipe_or_a_missing_file_is_refused() {
        let source_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let fifo = source_dir.path().join("fifo.deb");
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `c_path` is a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        for source in [
            source_dir.path().to_path_buf(),
            fifo,
            source_dir.path().join("missing.deb"),
        ] {
            let err = StagedCopy::copy(&source, parent.path()).err().unwrap();
            assert!(
                matches!(err, Refusal::File { .. } | Refusal::FileUnreadable { .. }),
                "{source:?}: {err:?}"
            );
        }
    }

    #[test]
    fn a_copy_past_the_limit_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let from_path = dir.path().join("from");
        std::fs::write(&from_path, [0u8; 100]).unwrap();
        let mut from = File::open(&from_path).unwrap();
        let mut to = File::create(dir.path().join("to")).unwrap();
        assert!(matches!(
            copy_hashing(&mut from, &mut to, 99),
            Err(CopyError::TooLarge)
        ));
    }
}
