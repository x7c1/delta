//! [`NullCommsLog`]: the comms log sink that records nothing.

use std::sync::Arc;

use super::{CommsEntry, CommsLogSink};

/// The sink for a build with nothing observing: every call is a no-op.
///
/// Lets an adapter hold a non-optional `Arc<dyn CommsLogSink>` — no `Option`
/// dance and no `if let` around every emit — and is what a test or a
/// composition root that wires no inspector installs. Mirrors
/// [`NullContentSource`](crate::NullContentSource).
#[derive(Debug, Clone, Copy, Default)]
pub struct NullCommsLog;

impl NullCommsLog {
    /// This sink behind an `Arc`, the form the adapters hold.
    pub fn arc() -> Arc<dyn CommsLogSink> {
        Arc::new(Self)
    }
}

impl CommsLogSink for NullCommsLog {
    fn record(&self, _session_id: &str, _entry: CommsEntry) {}

    fn discard(&self, _session_id: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{CommsDirection, CommsFrameKind};

    /// The null sink accepts both operations without panicking — the property
    /// every adapter relies on when no inspector is wired.
    #[test]
    fn the_null_sink_swallows_every_call() {
        let sink = NullCommsLog::arc();
        sink.record(
            "sess-1",
            CommsEntry::new(
                CommsDirection::ToAgent,
                CommsFrameKind::Request,
                Some("thread/start"),
                "{}",
            ),
        );
        sink.discard("sess-1");
    }
}
