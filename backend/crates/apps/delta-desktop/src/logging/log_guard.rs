use std::path::PathBuf;
use std::sync::Mutex;

use tracing_appender::non_blocking::WorkerGuard;

/// Keeps the log file's worker alive until [`flush`](Self::flush).
pub struct LogGuard {
    pub(super) worker: Mutex<Option<WorkerGuard>>,
    /// The directory the file is written in, when it is.
    pub(super) dir: Option<PathBuf>,
}

impl LogGuard {
    /// Write out what the file's worker still holds and stop it. Lines logged
    /// afterwards go to stdout only. Calling it again does nothing.
    pub fn flush(&self) {
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        drop(worker);
    }
}
