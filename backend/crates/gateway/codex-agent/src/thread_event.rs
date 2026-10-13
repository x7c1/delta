//! A frame (or the end of frames) on a per-thread channel.

use crate::wire::{Notification, ServerRequest};

/// A frame delivered to a per-thread channel by the demux — or the terminal
/// fact that no further frame can arrive.
///
/// The two frame variants are deliberately not translated into a neutral
/// `AgentEvent` here: the transport hands the adapter the raw, thread-scoped
/// server frames so it can do the translation.
///
/// [`ThreadEvent::ConnectionLost`] is not a frame at all. It rides the same
/// per-thread channel so it is ordered *behind* every frame that did arrive:
/// the adapter's translation task therefore sees a thread's last real frames
/// (an approval request the server managed to write before dying, say) before
/// it learns the connection is gone, and the core settles in that order.
#[derive(Debug, Clone, PartialEq)]
pub enum ThreadEvent {
    /// A server → client notification for this thread (`item/*`, `turn/*`, …).
    Notification(Notification),
    /// A server → client request for this thread (`*/requestApproval`), still
    /// awaiting a response the adapter will send.
    ServerRequest(ServerRequest),
    /// The connection carrying this thread died: the reader saw EOF (the
    /// `codex app-server` process exited) or a read error, so nothing further
    /// will ever arrive on this channel and no outgoing frame can be answered.
    ///
    /// Always the last event a thread receives, and emitted exactly once per
    /// thread — the connection's reader announces it as it exits. It
    /// exists so a dead connection is a *fact on the stream* rather than the
    /// stream merely going quiet: silence is indistinguishable from a slow
    /// model, and that is precisely how a stuck turn and an unanswerable
    /// approval dialog survived forever in the field.
    ConnectionLost,
}
