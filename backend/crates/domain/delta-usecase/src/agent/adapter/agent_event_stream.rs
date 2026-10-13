//! The stream of [`AgentEvent`]s for one session.

use tokio::sync::mpsc::UnboundedReceiver;

use crate::agent::AgentEvent;

/// The stream of [`AgentEvent`]s for one session.
///
/// A thin wrapper over an unbounded receiver, so callers depend on this crate's
/// surface rather than reaching for the channel type directly. Each session's
/// stream is handed out once; a second request yields an already-closed stream.
#[derive(Debug)]
pub struct AgentEventStream {
    rx: UnboundedReceiver<AgentEvent>,
}

impl AgentEventStream {
    /// Wrap a receiver as an event stream.
    pub fn new(rx: UnboundedReceiver<AgentEvent>) -> Self {
        Self { rx }
    }

    /// Receive the next event, or `None` once the session's sender is gone and
    /// the buffer is drained.
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        self.rx.recv().await
    }
}
