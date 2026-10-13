//! A live, bound pane-backed session.

use crate::pane_token::PaneToken;

/// A live, bound session: its Claude `session_id` is known and it is mapped to
/// the tmux pane driving it.
#[derive(Debug, Clone)]
pub struct OpenHandle {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to and the PTY attaches to (`<token>:0.0`).
    pub pane: String,
    /// Whether the agent in this pane was launched with hook URLs that no
    /// longer reach this server — a pane re-adopted after a restart whose hook
    /// endpoint changed (see [`crate::ports::RememberedPane::hooks_unreachable`]).
    /// Always `false` for a pane this process launched. Dropped with the
    /// handle when the session closes, which is what the browser's notice
    /// tells the user to do.
    pub hooks_unreachable: bool,
}

impl OpenHandle {
    /// The handle of a pane this process just launched for the session: its
    /// hooks point at this server, so they are reachable by construction.
    pub fn launched(token: PaneToken, pane: String) -> Self {
        Self {
            token,
            pane,
            hooks_unreachable: false,
        }
    }

    /// The record of this handle the session row keeps, so a restarted Delta
    /// can find the pane again.
    pub fn remembered(&self) -> crate::ports::RememberedPane {
        crate::ports::RememberedPane {
            tmux_session: self.token.as_str().to_owned(),
            pane: self.pane.clone(),
            hooks_unreachable: self.hooks_unreachable,
        }
    }
}
