use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Why the downloaded update was not copied and checked
/// ([`copy_verified`]).
#[derive(Debug, thiserror::Error)]
pub enum CopyError {
    /// The update file is a symlink, refused without being followed.
    #[error("the update file {} is a symlink", file.display())]
    Symlink { file: PathBuf },
    /// The update file is not a regular file (a directory, a device, a
    /// pipe), refused without being read.
    #[error("the update file {} is not a regular file", file.display())]
    NotRegularFile { file: PathBuf },
    /// The update file could not be opened or read.
    #[error("could not read the update file {}: {source}", file.display())]
    Unreadable {
        file: PathBuf,
        #[source]
        source: io::Error,
    },
    /// What was copied is not the file the download was verified as.
    #[error("the update file's sha256 is {actual}, not the {expected} it was downloaded with")]
    DigestMismatch { expected: String, actual: String },
    /// The copy could not be written: the working directory is full or
    /// out of reach. Nothing is wrong with the update file itself.
    #[error("could not write the copy of the update file to {}: {source}", copy.display())]
    Unwritable {
        copy: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl CopyError {
    /// Whether the update file itself is not one to install, by any means:
    /// every failure but [`Self::Unwritable`], which is about where the copy
    /// was to be written.
    pub fn rejects_file(&self) -> bool {
        !matches!(self, Self::Unwritable { .. })
    }
}

/// Copy the downloaded update at `file` to `copy`, a new file in a
/// directory only this user can reach, and check that what was copied has
/// the lowercase hex `sha256`, the digest the download was verified with.
///
/// The download sits in a directory the user owns, so it is checked again
/// rather than trusted, and everything after this reads the copy, which
/// cannot be swapped after the check. A symlink or anything but a regular
/// file is refused without being read.
pub fn copy_verified(file: &Path, sha256: &str, copy: &Path) -> Result<(), CopyError> {
    let unreadable = |source: io::Error| CopyError::Unreadable {
        file: file.to_path_buf(),
        source,
    };
    let mut source = OpenOptions::new()
        .read(true)
        // Never follow a symlink, and never wait on a FIFO to open.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(file)
        .map_err(|err| match err.raw_os_error() {
            Some(libc::ELOOP) => CopyError::Symlink {
                file: file.to_path_buf(),
            },
            _ => unreadable(err),
        })?;
    let metadata = source.metadata().map_err(unreadable)?;
    if !metadata.is_file() {
        return Err(CopyError::NotRegularFile {
            file: file.to_path_buf(),
        });
    }
    let actual = copy_hashing(&mut source, copy).map_err(|err| match err {
        Side::Read(source) => unreadable(source),
        Side::Write(source) => CopyError::Unwritable {
            copy: copy.to_path_buf(),
            source,
        },
    })?;
    if !actual.eq_ignore_ascii_case(sha256) {
        return Err(CopyError::DigestMismatch {
            expected: sha256.to_owned(),
            actual,
        });
    }
    Ok(())
}

/// Which end of a copy failed: reading the source or writing the copy.
enum Side {
    Read(io::Error),
    Write(io::Error),
}

/// Copy `source` into a new file at `copy` and return the sha256 of what was
/// copied, lowercase hex.
fn copy_hashing(source: &mut File, copy: &Path) -> Result<String, Side> {
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(copy)
        .map_err(Side::Write)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = source.read(&mut buffer).map_err(Side::Read)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        target.write_all(&buffer[..read]).map_err(Side::Write)?;
    }
    target.sync_all().map_err(Side::Write)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn a_matching_file_is_copied() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Delta_0.6.0_aarch64.dmg");
        std::fs::write(&file, b"the release").unwrap();
        let copy = dir.path().join("copy.dmg");
        copy_verified(&file, &sha256_hex(b"the release"), &copy).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"the release");
    }

    #[test]
    fn a_digest_mismatch_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Delta_0.6.0_aarch64.dmg");
        std::fs::write(&file, b"another file").unwrap();
        let expected = sha256_hex(b"the release");
        let err = copy_verified(&file, &expected, &dir.path().join("copy.dmg")).unwrap_err();
        let actual = sha256_hex(b"another file");
        assert!(
            matches!(&err, CopyError::DigestMismatch { expected: e, actual: a } if *e == expected && *a == actual),
            "{err:?}"
        );
        assert!(err.rejects_file());
    }

    #[test]
    fn a_symlink_or_a_directory_is_rejected_unread() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.dmg");
        std::fs::write(&real, b"the release").unwrap();
        let link = dir.path().join("Delta_0.6.0_aarch64.dmg");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let copy = dir.path().join("copy.dmg");
        let err = copy_verified(&link, &sha256_hex(b"the release"), &copy).unwrap_err();
        assert!(
            matches!(&err, CopyError::Symlink { file } if *file == link),
            "{err:?}"
        );
        assert!(err.rejects_file());
        assert!(!copy.exists());

        let directory = dir.path().join("a-directory.dmg");
        std::fs::create_dir(&directory).unwrap();
        let err = copy_verified(&directory, &sha256_hex(b""), &copy).unwrap_err();
        assert!(
            matches!(&err, CopyError::NotRegularFile { file } if *file == directory),
            "{err:?}"
        );
        assert!(err.rejects_file());
        assert!(!copy.exists());
    }

    #[test]
    fn a_copy_that_cannot_be_written_does_not_reject_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Delta_0.6.0_aarch64.dmg");
        std::fs::write(&file, b"the release").unwrap();
        let copy = dir.path().join("missing-directory").join("copy.dmg");
        let err = copy_verified(&file, &sha256_hex(b"the release"), &copy).unwrap_err();
        assert!(
            matches!(&err, CopyError::Unwritable { copy: c, .. } if *c == copy),
            "{err:?}"
        );
        assert!(!err.rejects_file());
    }
}
