//! One frame an adapter hands to the comms log.

use super::{CommsDirection, CommsFrameKind};

#[cfg(doc)]
use super::CommsLogSink;

/// One frame as the adapter hands it over, before the sink stamps it.
///
/// The sink owns the sequence number and the timestamp (see
/// [`CommsLogSink::record`]) so ordering is decided in one place rather than by
/// whichever task happened to emit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommsEntry {
    /// Which way the frame travelled.
    pub direction: CommsDirection,
    /// What kind of message it is.
    pub kind: CommsFrameKind,
    /// The method name, for the kinds that name one (requests and
    /// notifications). A response is identified by the request it answers, so
    /// the adapter fills this with *that* request's method where it knows it,
    /// and `None` where it does not.
    pub method: Option<String>,
    /// The frame itself, as JSON text. Exactly the bytes written for an
    /// outgoing frame; for an incoming one, the frame as the transport parsed it
    /// (so a top-level field the transport ignores — a `jsonrpc` version tag —
    /// is not shown, while everything nested under `params`/`result` is).
    pub payload_json: String,
}

impl CommsEntry {
    /// Build an entry, taking the method as a borrowed name (the common case at
    /// the call sites, which have a `&str` in hand).
    pub fn new(
        direction: CommsDirection,
        kind: CommsFrameKind,
        method: Option<&str>,
        payload_json: impl Into<String>,
    ) -> Self {
        Self {
            direction,
            kind,
            method: method.map(str::to_owned),
            payload_json: payload_json.into(),
        }
    }
}
