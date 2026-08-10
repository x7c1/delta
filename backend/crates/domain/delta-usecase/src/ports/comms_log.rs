//! [`CommsLogSink`]: the observability tap for the frames Delta exchanges with
//! an agent it drives over a structured transport.
//!
//! ## Why this exists
//!
//! A provider Delta launches as a terminal program has a window into "what is
//! the agent doing right now": its PTY. A provider Delta drives *headlessly*
//! over a structured transport (Codex's `codex app-server` JSON-RPC) has no such
//! window — the only externally visible artefacts are the conversation events
//! the adapter chose to translate. When something goes wrong one level below
//! that (a field the server never populates, an approval request Delta does not
//! model), there is nothing to look at.
//!
//! This port is that window: the adapter hands every frame it writes and every
//! frame it reads to a sink, which the transport layer streams to the browser.
//!
//! ## What this is NOT
//!
//! **Observability only.** A recorded frame is not conversation: it never
//! becomes an [`AgentEvent`](crate::AgentEvent), never reaches the persistence
//! pipeline or attribution, and is never written to the database. It exists to
//! be *looked at* while a session is live, and losing it (a server restart, a
//! buffer that wrapped) costs nothing but the view.
//!
//! ## The contract implementations must honour
//!
//! [`CommsLogSink::record`] is called from inside the adapter's send and
//! receive paths — on the very code path a turn's progress depends on — so it
//! **must not block**. An implementation buffers with a bound and drops the
//! oldest (or the slowest consumer's copy) rather than making the caller wait:
//! a browser that stopped reading, or no browser at all, must never be able to
//! stall a turn. Never letting a session hang invisibly is why a headless
//! provider has no terminal in the first place, so it is the invariant this port
//! protects.

use std::sync::Arc;

/// Which way one recorded frame travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommsDirection {
    /// Delta → agent: a frame Delta wrote.
    ToAgent,
    /// Agent → Delta: a frame Delta read.
    FromAgent,
}

/// What kind of message one recorded frame is.
///
/// Deliberately three variants, not four: a *server-originated request* is not
/// a different kind of message from a request — it is a request travelling the
/// other way. Every frame shape a JSON-RPC-style transport carries is therefore
/// a (direction, kind) pair:
///
/// | frame | direction | kind |
/// |---|---|---|
/// | client request | [`CommsDirection::ToAgent`] | [`CommsFrameKind::Request`] |
/// | client notification | `ToAgent` | [`CommsFrameKind::Notification`] |
/// | client response (answering a server request) | `ToAgent` | [`CommsFrameKind::Response`] |
/// | server response | [`CommsDirection::FromAgent`] | `Response` |
/// | server request | `FromAgent` | `Request` |
/// | server notification | `FromAgent` | `Notification` |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommsFrameKind {
    /// Carries an id and awaits a correlated response.
    Request,
    /// Answers a request, carrying its id.
    Response,
    /// Fire and forget: no id, no answer.
    Notification,
}

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

/// Where an adapter hands the frames it exchanges with its provider.
///
/// Provider-neutral on purpose: an adapter whose provider has no inspectable
/// wire (a terminal program) simply never calls [`Self::record`], so no
/// per-provider branch is needed anywhere. See the module docs for the
/// non-blocking contract.
pub trait CommsLogSink: Send + Sync {
    /// Record one frame against the Delta session it belongs to.
    ///
    /// `session_id` is **Delta's** conversation id, not the provider's — the
    /// browser asks for a session's log by the id it already knows, and the
    /// adapter is the only layer that can map its own transport scoping onto
    /// it. A frame that belongs to no session (a shared transport's handshake,
    /// which precedes every session) is simply not recorded: it has no
    /// inspector to appear in.
    ///
    /// The implementation stamps the ordering (sequence, timestamp). Must not
    /// block — see the module docs.
    fn record(&self, session_id: &str, entry: CommsEntry);

    /// Drop everything held for `session_id`.
    ///
    /// Called when a session ends, so the per-session buffers do not accumulate
    /// for the process's lifetime. A closed session has no live wire, so its log
    /// has nothing left to say — the inspector shows its idle state instead.
    fn discard(&self, session_id: &str);
}

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
