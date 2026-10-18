use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

/// The socket this copy listens on. [`release`](Self::release) removes it when
/// the app exits.
#[derive(Debug)]
pub struct InstanceLock {
    path: PathBuf,
}

impl InstanceLock {
    /// The lock held through the socket this copy bound at `path`.
    pub(super) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Remove the socket, so the next launch finds no copy running and does
    /// not have to take over a stale socket. Calling it again does nothing.
    pub fn release(&self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(
                socket = %self.path.display(),
                "could not remove the single-instance socket: {err}"
            ),
        }
    }
}
