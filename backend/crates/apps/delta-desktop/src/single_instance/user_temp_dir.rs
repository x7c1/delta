use std::ffi::{CStr, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use anyhow::{bail, Context};

/// The per-user temporary directory macOS assigns this user
/// (`confstr(_CS_DARWIN_USER_TEMP_DIR)`, under `/var/folders`): the directory
/// `$TMPDIR` normally names, owned by the user with mode 0700. It is asked of
/// the system rather than read from `$TMPDIR`, which whoever starts the app can
/// point anywhere; macOS creates it if it is missing.
pub fn user_temp_dir() -> anyhow::Result<PathBuf> {
    let name = libc::_CS_DARWIN_USER_TEMP_DIR;
    // SAFETY: a null buffer of length 0 only asks for the size needed.
    let needed = unsafe { libc::confstr(name, std::ptr::null_mut(), 0) };
    if needed == 0 {
        return Err(io::Error::last_os_error())
            .context("confstr(_CS_DARWIN_USER_TEMP_DIR) gave no directory");
    }
    let mut buf = vec![0u8; needed];
    // SAFETY: `buf` is writable for `needed` bytes, the length passed.
    let written = unsafe { libc::confstr(name, buf.as_mut_ptr().cast(), buf.len()) };
    if written == 0 || written > buf.len() {
        bail!("confstr(_CS_DARWIN_USER_TEMP_DIR) changed its answer while it was read");
    }
    let dir = CStr::from_bytes_until_nul(&buf)
        .context("confstr(_CS_DARWIN_USER_TEMP_DIR) gave no terminated string")?;
    if dir.is_empty() {
        bail!("confstr(_CS_DARWIN_USER_TEMP_DIR) gave an empty directory");
    }
    Ok(PathBuf::from(OsStr::from_bytes(dir.to_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directory_is_an_existing_private_one() {
        let dir = user_temp_dir().unwrap();
        assert!(dir.is_absolute(), "{}", dir.display());
        super::super::ensure_private_dir::ensure_private_dir(&dir).unwrap();
    }
}
