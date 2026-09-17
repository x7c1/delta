//! [`SessionListingState`]: the runtime facts one session-list row carries,
//! read in a single actor message.

use super::SessionRuntime;

/// The process-runtime facts the session list annotates a stored row with,
/// snapshotted in one read so the row can never report a pair that never
/// existed (e.g. `open` from before a bind and `pane_starting` from after it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionListingState {
    /// Whether the session is open: a live, bound pane (Claude) or a live
    /// terminal-less agent session (Codex).
    pub open: bool,
    /// Whether the session holds a pane the PTY bridge may attach to that
    /// nothing has bound yet — a launch whose tmux pane is up while its first
    /// hook has not arrived.
    ///
    /// Deliberately read off the same pane the `/pty` bridge resolves against
    /// ([`SessionRuntime::attachable_pane`]), so an attach while this is `true`
    /// succeeds. Note it is narrower than
    /// [`SessionRuntime::has_live_pane`], which is also true while the launch
    /// is still preparing and no pane exists yet.
    pub pane_starting: bool,
}

impl SessionRuntime {
    /// Snapshot this session's [`SessionListingState`].
    pub fn listing_state(&self) -> SessionListingState {
        SessionListingState {
            open: self.is_open(),
            pane_starting: self.attachable_pane().is_some_and(|pane| !pane.bound),
        }
    }
}
