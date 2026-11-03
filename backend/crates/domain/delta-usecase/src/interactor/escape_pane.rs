//! Pressing `Escape` in a session's pane on the user's behalf.
//!
//! Several controls do what a human would at the keyboard: cancelling a
//! dispatched send whose keystrokes still sit in the composer
//! ([`cancel_send`](super::cancel_send)), clearing the pane before an
//! echo-deadline re-type ([`echo_deadline`](super::echo_deadline)), and
//! stopping a running Claude turn ([`stop_turn`](super::stop_turn)). They all
//! inject the same single keystroke — the pinned [`cancel_keys`] sequence — so
//! it is built in one place.
//!
//! Stopping a turn also has to leave the composer empty, which the Escape alone
//! does not: when Claude Code drops a prompt that had submitted but drew no
//! reply yet, it restores that prompt into the composer, and the Escape does
//! not clear it, so stopping a turn presses `Ctrl+C` after the `Escape`.

use crate::error::Result;
use crate::interactor::question_keys::cancel_keys;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Inject a single `Escape` keystroke into `pane`.
    pub(in crate::interactor) async fn press_escape(&self, pane: &str) -> Result<()> {
        let keys = cancel_keys();
        let names: Vec<&str> = keys.iter().map(|key| key.tmux_name()).collect();
        self.tmux.send_keys(pane, &names).await
    }

    /// Inject `Escape`, then `Ctrl+C`, into `pane`: interrupt whatever Claude
    /// is doing and leave its composer empty.
    ///
    /// The `Escape` interrupts a streaming reply. The `Ctrl+C` clears the
    /// composer whole, however many lines it holds and wherever its cursor
    /// sits — unlike `C-u`, which clears only the cursor's line — because
    /// Claude Code (verified on 2.1.316) restores a prompt it dropped before
    /// any reply streamed into the composer, and the next send typed there
    /// would be spliced into it. On an already empty composer Claude Code
    /// takes the `Ctrl+C` as the first of the two presses that exit it, and
    /// shows a hint for about two seconds; the next keystroke (such as the
    /// queued send Delta types next) disarms it.
    ///
    /// The two keys must reach the TUI as separate keystrokes:
    /// [`TmuxDriver::send_keys`] sends each one on its own, whereas an `ESC`
    /// byte immediately followed by `0x03` is read as `Meta+C` and neither
    /// interrupts nor clears.
    ///
    /// [`TmuxDriver::send_keys`]: crate::ports::TmuxDriver::send_keys
    pub(in crate::interactor) async fn press_escape_and_clear_composer(
        &self,
        pane: &str,
    ) -> Result<()> {
        let mut names: Vec<&str> = cancel_keys().iter().map(|key| key.tmux_name()).collect();
        names.push(CLEAR_COMPOSER_KEY);
        self.tmux.send_keys(pane, &names).await
    }
}

/// The tmux `send-keys` name of the keystroke that empties Claude Code's
/// composer (`Ctrl+C`).
const CLEAR_COMPOSER_KEY: &str = "C-c";
