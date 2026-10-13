//! The result of [`AppServerConnection::start_thread`].

use serde_json::Value;
use tokio::sync::mpsc;

use crate::ThreadEvent;

#[cfg(doc)]
use crate::AppServerConnection;

/// The result of [`AppServerConnection::start_thread`].
pub struct StartedThread {
    /// The provider's thread id (Codex's `thr_...`).
    pub thread_id: String,
    /// The thread's event channel — server notifications and server requests
    /// scoped to this thread arrive here.
    pub events: mpsc::UnboundedReceiver<ThreadEvent>,
    /// The raw `thread/start` result, for any provider-specific fields the
    /// caller wants beyond the thread id.
    pub result: Value,
}
