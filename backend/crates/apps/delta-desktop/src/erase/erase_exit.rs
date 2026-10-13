//! Managed state marking an exit as the end of an erase.

use std::sync::atomic::{AtomicBool, Ordering};

/// Managed state: whether the app is quitting because the server erased
/// everything, so the per-app directories go when the process ends.
#[derive(Default)]
pub struct EraseExit(AtomicBool);

impl EraseExit {
    /// Record that this exit is an erase.
    pub fn mark(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether [`mark`](Self::mark) was called.
    pub fn is_marked(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
