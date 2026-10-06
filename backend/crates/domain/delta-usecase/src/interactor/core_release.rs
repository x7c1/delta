//! [`CoreRelease`]: telling when an interactor's ports have been dropped.

use std::any::Any;
use std::sync::Weak;
use std::time::Duration;

/// How often [`CoreRelease::released_within`] looks again.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Reports when an [`Interactor`](super::Interactor)'s shared core — and with
/// it every port, the store's database connection included — has been
/// dropped.
///
/// Dropping the interactor alone is not enough to say that: each session
/// actor holds the core until it has run down, which it does on its own task
/// once its mailbox closes. A caller that must not act until the store is
/// closed (deleting the database file) waits on this instead.
pub struct CoreRelease(Weak<dyn Any + Send + Sync>);

impl CoreRelease {
    pub(super) fn new(core: Weak<dyn Any + Send + Sync>) -> Self {
        Self(core)
    }

    /// Whether the core has been dropped.
    pub fn is_released(&self) -> bool {
        self.0.strong_count() == 0
    }

    /// Wait until the core has been dropped, for at most `limit`, returning
    /// whether it was.
    pub async fn released_within(&self, limit: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + limit;
        while !self.is_released() {
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        true
    }
}
