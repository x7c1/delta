//! One ordered log of calls shared across several fakes.

use std::sync::{Arc, Mutex};

/// The calls several fakes received, in the order they arrived.
///
/// Each fake records its own calls in its own vectors, which cannot show how
/// calls on *different* ports interleave — and some behaviour is exactly an
/// interleaving: erasing everything must kill the tmux server after closing
/// the sessions and before touching any worktree. Handing the same journal to
/// the fakes involved (`with_journal`) is what lets a test assert that order.
#[derive(Clone, Default)]
pub(crate) struct CallJournal(Arc<Mutex<Vec<String>>>);

impl CallJournal {
    /// Append one entry.
    pub(crate) fn record(&self, entry: impl Into<String>) {
        self.0.lock().unwrap().push(entry.into());
    }

    /// Every entry so far, in order.
    pub(crate) fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}
