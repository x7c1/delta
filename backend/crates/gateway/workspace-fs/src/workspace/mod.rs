//! [`FsWorkspace`]: the filesystem-backed [`Workspace`].
//!
//! Split by responsibility: this module holds the struct and the [`Workspace`]
//! trait wiring, `create_dir` the owner-only directory creation (a scratch
//! working directory, and the settings file's directory), `settings` the
//! hardened settings-file write, and `browse` the read-only directory queries
//! the picker needs.

mod browse;
mod create_dir;
mod settings;

use async_trait::async_trait;

use delta_usecase::{DirListing, Workspace};

/// Writes the Claude Code session settings file, creates the scratch working
/// directories sessions launch in, and browses local directories.
#[derive(Debug, Default, Clone)]
pub struct FsWorkspace;

impl FsWorkspace {
    /// Create a new filesystem workspace gateway.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Workspace for FsWorkspace {
    async fn write_session_settings(
        &self,
        settings_path: &str,
        settings_json: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.write(settings_path, settings_json)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn create_private_dir(
        &self,
        path: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.create_dir(path)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn resolve_existing_dir(
        &self,
        path: &str,
    ) -> std::result::Result<String, delta_usecase::Error> {
        let dir = self.resolve_dir(path).await?;
        Ok(dir.to_string_lossy().into_owned())
    }

    async fn list_dirs(
        &self,
        path: &str,
        include_hidden: bool,
    ) -> std::result::Result<DirListing, delta_usecase::Error> {
        Ok(self.list(path, include_hidden).await?)
    }
}
