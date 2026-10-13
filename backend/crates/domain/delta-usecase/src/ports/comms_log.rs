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

use super::CommsEntry;

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
