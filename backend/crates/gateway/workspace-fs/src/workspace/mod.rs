//! [`FsWorkspace`]: the filesystem-backed [`Workspace`].
//!
//! Split by responsibility: this module holds the struct and the [`Workspace`]
//! trait wiring, `create_dir` the owner-only directory creation (the settings
//! file's directory), `settings` the hardened settings-file write, `browse` the
//! read-only directory queries the picker needs, and `remove_dir` the deletion
//! of a leftover worktree directory.

mod browse;
mod create_dir;
mod remove_dir;
mod settings;

use std::time::SystemTime;

use async_trait::async_trait;

use delta_usecase::{DirListing, Workspace};

/// Writes the Claude Code session settings file and browses local
/// directories.
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

    async fn modified_at(
        &self,
        path: &str,
    ) -> std::result::Result<SystemTime, delta_usecase::Error> {
        Ok(self.modified(path).await?)
    }

    async fn remove_dir_tree(&self, path: &str) -> std::result::Result<(), delta_usecase::Error> {
        Ok(self.remove_tree(path).await?)
    }

    async fn remove_empty_dir(
        &self,
        path: &str,
    ) -> std::result::Result<bool, delta_usecase::Error> {
        Ok(self.remove_empty(path).await?)
    }
}
