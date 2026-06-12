use crate::error::Result;
use crate::pane_token::PaneToken;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Spawn a fresh Claude Code session with no initial send (cold-start).
    ///
    /// Mints a token, prepares a unique `<base>/<token>` working directory with
    /// the hook settings written into it, eagerly creates the session row
    /// (status `spawning`), launches `claude` in a new tmux session named after
    /// the token, and records a [`PendingSpawn`]. The first hook activates the
    /// row when it binds this spawn.
    ///
    /// [`PendingSpawn`]: crate::open_sessions::PendingSpawn
    pub async fn new_session(&self) -> Result<PaneToken> {
        Ok(self.spawn_fresh(None, None).await?.token)
    }
}
