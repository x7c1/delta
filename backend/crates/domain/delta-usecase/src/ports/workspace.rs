//! Preparing the on-disk working directory for the Claude Code session.

use async_trait::async_trait;

use crate::error::Result;

/// Prepares the on-disk working directory the Claude Code session runs in.
///
/// The session needs a working directory containing a `.claude/settings.json`
/// that points Claude Code's HTTP hooks back at this server. The server renders
/// that settings JSON (so the hook URLs always match the running port) and this
/// port persists it, creating the directory tree as needed.
#[async_trait]
pub trait Workspace: Send + Sync {
    /// Create `workdir` (and its `.claude` subdirectory) if absent and write
    /// `settings_json` to `<workdir>/.claude/settings.json`, overwriting any
    /// existing file so the hook URLs always reflect the current port.
    async fn write_session_settings(&self, workdir: &str, settings_json: &str) -> Result<()>;
}

#[async_trait]
impl Workspace for Box<dyn Workspace> {
    async fn write_session_settings(&self, workdir: &str, settings_json: &str) -> Result<()> {
        (**self).write_session_settings(workdir, settings_json).await
    }
}
