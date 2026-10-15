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
        self.write_session_settings().await
    }

    /// Write the session settings file and the files written next to it (see
    /// [`crate::SessionSettingsFile`]).
    ///
    /// The companions go first: the settings' command entries read them, so a
    /// settings file is never on disk ahead of the files it points at.
    pub(in crate::interactor) async fn write_session_settings(&self) -> Result<()> {
        for file in &self.session_settings_files {
            self.workspace
                .write_session_settings(&file.path, &file.contents)
                .await?;
        }
        self.workspace
            .write_session_settings(&self.session_settings_path, &self.session_settings_json)
            .await
    }
}
