//! What the PTY bridge may attach to, and who is attached right now.
//!
//! The pane a browser can reach is wider than the bound one: a fresh spawn's
//! pane exists from the moment `create_session` returns, a beat before anything
//! binds it, and that window is precisely when the launch may be sitting on an
//! interactive prompt only a human can answer. So the attach lookup reads both
//! the bound handle and the pending spawn, and reports which of the two it
//! found — the caller treats an unbound pane more carefully (see
//! [`AttachablePane::bound`]).

use std::time::Instant;

use super::SessionRuntime;

/// A pane the PTY bridge may attach to, and how far its session has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachablePane {
    /// The tmux target to attach to (`<token>:0.0`).
    pub pane: String,
    /// Whether the session is **bound** to that pane (open), as opposed to a
    /// fresh spawn whose pane is up but whose first hook has not arrived.
    ///
    /// Everything Delta types into a pane on the user's behalf assumes the bound
    /// state, where the pane is known to be an agent sitting at its prompt; an
    /// unbound pane may be showing anything. The one caller that acts on this is
    /// the PTY bridge, which skips its pre-attach input wipe when it is `false`
    /// (`delta_server::pty` explains why those keystrokes are unsafe there).
    pub bound: bool,
}

impl SessionRuntime {
    /// The pane the PTY bridge may attach to: the bound one, or a fresh spawn's
    /// pane that is up but not yet bound.
    ///
    /// `None` covers every state with nothing behind it — a session still
    /// preparing its launch (no pane exists yet), one whose launch failed, a
    /// closed session, and a terminal-less agent session, which has no pane at
    /// all. Those are exactly the cases the bridge refuses.
    pub fn attachable_pane(&self) -> Option<AttachablePane> {
        if let Some(handle) = self.handle() {
            return Some(AttachablePane {
                pane: handle.pane.clone(),
                bound: true,
            });
        }
        self.pending_spawn_pane()
            .map(|pane| AttachablePane { pane, bound: false })
    }

    /// Record that a PTY bridge attached to this session's pane.
    ///
    /// Counted rather than flagged: two browsers (or two tabs) can hold a bridge
    /// on the same pane at once, and the pane is in use until the last of them
    /// has gone.
    pub fn note_pty_attached(&mut self) {
        self.pty_attachments += 1;
    }

    /// Record that a PTY bridge detached, as of `now`.
    ///
    /// Saturating, so an unbalanced detach — a bridge whose session actor
    /// retired underneath it — reads as "nobody is attached" rather than
    /// wrapping into a pane nothing can ever reap.
    ///
    /// The **last** bridge leaving gives an unbound spawn its bind deadline
    /// afresh ([`SessionRuntime::restart_pending_deadline`]). While anybody is
    /// attached the watchdog will not reap the pane, but the deadline itself
    /// keeps running underneath — so without this, somebody who opened the
    /// terminal, read the dialog for a minute and then closed the column would
    /// have the session reaped on the very next sweep, as if closing the
    /// terminal had killed it. A pane somebody has just stepped away from gets
    /// the same grace as one nobody ever attached to.
    ///
    /// Only the transition to zero does that: with two tabs open, closing one
    /// leaves somebody watching and restarts nothing. Neither does a detach
    /// that balances no attach — the deadline of a spawn nobody ever reached is
    /// not something a stray decrement may extend.
    ///
    /// `now` is supplied by the caller rather than read here, like the watchdog
    /// drains it competes with, so the whole attach/detach/reap sequence is
    /// deterministic under test.
    pub fn note_pty_detached(&mut self, now: Instant) {
        if self.pty_attachments == 0 {
            return;
        }
        self.pty_attachments -= 1;
        if self.pty_attachments == 0 {
            self.restart_pending_deadline(now);
        }
    }

    /// Whether anybody is watching this session's pane through a PTY bridge
    /// right now. Read by [`SessionRuntime::take_stale_pending`], which will not
    /// reap a pane somebody is attached to.
    pub fn has_pty_attachment(&self) -> bool {
        self.pty_attachments > 0
    }
}
