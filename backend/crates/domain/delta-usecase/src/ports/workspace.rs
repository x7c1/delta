//! Persisting the Claude Code session settings file.

use async_trait::async_trait;

use crate::error::Result;

/// Persists the settings file that Delta hands to `claude --settings <file>`.
///
/// The session needs a settings file that points Claude Code's HTTP hooks back
/// at this server (and pins the session theme). The server renders that JSON (so
/// the hook URLs always match the running port) and this port writes it to a
/// Delta-owned path, creating parent directories as needed. The path is
/// deliberately *outside* the session's working directory, so spawning or
/// resuming in a real project never clobbers that project's own
/// `.claude/settings.json`.
#[async_trait]
pub trait Workspace: Send + Sync {
    /// Write `settings_json` to `settings_path`, creating parent directories if
    /// absent and overwriting any existing file so the hook URLs always reflect
    /// the current port.
    async fn write_session_settings(
        &self,
        settings_path: &str,
        settings_json: &str,
    ) -> Result<()>;
}

#[async_trait]
impl Workspace for Box<dyn Workspace> {
    async fn write_session_settings(
        &self,
        settings_path: &str,
        settings_json: &str,
    ) -> Result<()> {
        (**self)
            .write_session_settings(settings_path, settings_json)
            .await
    }
}
