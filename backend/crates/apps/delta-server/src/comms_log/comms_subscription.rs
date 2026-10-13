//! One watcher's ordered view of a session's log: the replay, then the tail.

use std::collections::VecDeque;

use delta_wire::WireCommsFrame;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

/// One watcher's ordered view of a session's log: the replay, then the tail.
///
/// Deliberately a type of its own rather than a bare receiver, so the
/// replay-then-tail contract is expressed once and testable without a WebSocket
/// (the `/comms` handler does nothing but pump this into a socket).
pub struct CommsSubscription {
    /// The buffered frames still to be delivered, oldest first.
    replay: VecDeque<WireCommsFrame>,
    /// The live tail, joined at the instant the replay was snapshotted.
    live: broadcast::Receiver<WireCommsFrame>,
}

impl CommsSubscription {
    /// A subscription that delivers `replay` first, then whatever `live`
    /// carries from the instant the replay was snapshotted.
    pub(super) fn new(
        replay: VecDeque<WireCommsFrame>,
        live: broadcast::Receiver<WireCommsFrame>,
    ) -> Self {
        Self { replay, live }
    }

    /// The next frame to show, or `None` once the session's log is gone (the
    /// session ended, or the server is shutting down).
    ///
    /// A subscriber that fell behind the broadcast bound is skipped forward with
    /// a warning rather than closed: a gap in an observability stream is
    /// survivable, and closing would lose the frames still coming.
    pub async fn next(&mut self) -> Option<WireCommsFrame> {
        if let Some(frame) = self.replay.pop_front() {
            return Some(frame);
        }
        loop {
            match self.live.recv().await {
                Ok(frame) => return Some(frame),
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "comms log subscriber lagged");
                }
                Err(RecvError::Closed) => return None,
            }
        }
    }
}
