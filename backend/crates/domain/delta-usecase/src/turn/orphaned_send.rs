//! A send abandoned by a transition, with what the caller must do about it.

/// A send abandoned by a transition, with what the caller must do about it.
/// See the `turn` module docs for the disposition semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanedSend {
    /// Return the send to `queued`: no prompt was ever submitted for it, so it
    /// re-dispatches intact when the session next goes idle.
    Requeue(i64),
    /// Cancel the send: it can never be delivered.
    Cancel(i64),
    /// Settle the send as delivered (`matched`, no uuid) if it is still
    /// `dispatched`: its turn ran and has ended, so whether or not a transcript
    /// line ever claimed it, the message went out.
    SettleIfUnmatched(i64),
}
