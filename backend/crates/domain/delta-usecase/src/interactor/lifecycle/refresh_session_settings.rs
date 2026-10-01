//! Rewriting the session settings file at startup.

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Write the session settings file now, rather than only before the next
    /// launch.
    ///
    /// Run once when the server starts serving. A Claude Code session that
    /// survived the restart in tmux was launched with `--settings <path>`, and
    /// the path and hook URLs are kept stable across restarts, so this run's
    /// rendering lands on the same file. Rewriting it straight away means the
    /// file on disk always matches the running server — never a stale copy
    /// from a run with different values, left until the next spawn or resume
    /// happens to overwrite it.
    pub async fn refresh_session_settings(&self) -> Result<()> {
        self.workspace
            .write_session_settings(&self.session_settings_path, &self.session_settings_json)
            .await
    }
}
