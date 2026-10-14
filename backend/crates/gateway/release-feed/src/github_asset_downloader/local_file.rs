//! Reading and clearing the files in the update directory, which is the
//! user's to write to: nothing there is trusted for having been checked
//! before.

use std::io::ErrorKind;
use std::path::Path;

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use delta_usecase::AssetDownloadError;

use super::to_hex;

/// Open the regular file at `path` for reading, or `None` when there is
/// nothing at `path`.
///
/// A symlink is refused without being followed, and anything but a regular
/// file without being read; a FIFO does not block the open.
async fn open_regular_file(path: &Path) -> Result<Option<tokio::fs::File>, AssetDownloadError> {
    let not_regular = || AssetDownloadError::NotRegularFile {
        path: path.to_path_buf(),
    };
    let opened = tokio::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .await;
    let file = match opened {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) if err.raw_os_error() == Some(libc::ELOOP) => return Err(not_regular()),
        Err(err) => return Err(unreadable(path, err)),
    };
    if !file
        .metadata()
        .await
        .map_err(|err| unreadable(path, err))?
        .is_file()
    {
        return Err(not_regular());
    }
    Ok(Some(file))
}

fn unreadable(path: &Path, source: std::io::Error) -> AssetDownloadError {
    AssetDownloadError::Unreadable {
        path: path.to_path_buf(),
        source,
    }
}

/// The sha256 of the regular file at `path` as lowercase hex, feeding each
/// chunk read to `feed` too, or `None` when there is nothing at `path`
/// (refused as [`open_regular_file`] refuses).
pub(super) async fn sha256_of_regular_file(
    path: &Path,
    feed: &mut (dyn FnMut(&[u8]) + Send),
) -> Result<Option<String>, AssetDownloadError> {
    let Some(mut file) = open_regular_file(path).await? else {
        return Ok(None);
    };
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|err| unreadable(path, err))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        feed(&buffer[..read]);
    }
    Ok(Some(to_hex(&hasher.finalize())))
}

/// The contents of the regular file at `path`, read up to `limit` bytes and
/// one more, so a caller can tell a larger file, or `None` when there is
/// nothing at `path` (refused as [`open_regular_file`] refuses).
pub(super) async fn read_regular_file(
    path: &Path,
    limit: usize,
) -> Result<Option<Vec<u8>>, AssetDownloadError> {
    let Some(file) = open_regular_file(path).await? else {
        return Ok(None);
    };
    let mut contents = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut contents)
        .await
        .map_err(|err| unreadable(path, err))?;
    Ok(Some(contents))
}

/// Whether there is anything at `path`, not following a symlink.
pub(super) async fn exists(path: &Path) -> Result<bool, AssetDownloadError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(unreadable(path, err)),
    }
}

/// Remove the file at `path`, if there is one. A failure is logged with
/// `failure` as its message.
pub(super) async fn remove_logged(path: &Path, failure: &str) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(
            path = %path.display(),
            error = %err,
            "{failure}"
        ),
    }
}

/// Remove every file in `dir` but those named in `keep`: the files of other
/// versions, and any partial download an earlier run left behind. A missing
/// `dir` has nothing to remove; any other failure is logged and the rest are
/// still removed.
pub(super) async fn remove_others(dir: &Path, keep: &[&str]) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return,
        Err(err) => {
            tracing::warn!(
                dir = %dir.display(),
                error = %err,
                "could not list the update directory to remove older downloads"
            );
            return;
        }
    };
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(
                    dir = %dir.display(),
                    error = %err,
                    "could not list the update directory to remove older downloads"
                );
                break;
            }
        };
        if keep.iter().any(|name| entry.file_name() == **name) {
            continue;
        }
        let path = entry.path();
        if let Err(err) = tokio::fs::remove_file(&path).await {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "could not remove an older download"
            );
        }
    }
}
