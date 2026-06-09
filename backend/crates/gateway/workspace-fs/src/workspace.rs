//! [`FsWorkspace`]: the filesystem-backed [`Workspace`].

use std::path::Path;

use async_trait::async_trait;

use delta_usecase::Workspace;

use crate::error::Error;

/// Writes the Claude Code session settings file on the local filesystem.
#[derive(Debug, Default, Clone)]
pub struct FsWorkspace;

impl FsWorkspace {
    /// Create a new filesystem workspace gateway.
    pub fn new() -> Self {
        Self
    }

    async fn write(&self, settings_path: &str, settings_json: &str) -> Result<(), Error> {
        let path = Path::new(settings_path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, settings_json).await?;
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_settings_creating_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        // A nested path whose parents do not exist yet: the write must create them.
        let settings_path = dir.path().join("run").join("settings.json");
        let settings_path_str = settings_path.to_str().unwrap();

        let ws = FsWorkspace::new();
        ws.write_session_settings(settings_path_str, r#"{"hooks":{}}"#)
            .await
            .unwrap();

        let written = tokio::fs::read_to_string(&settings_path).await.unwrap();
        assert_eq!(written, r#"{"hooks":{}}"#);
    }

    #[tokio::test]
    async fn overwrites_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        let settings_path_str = settings_path.to_str().unwrap();
        let ws = FsWorkspace::new();

        ws.write_session_settings(settings_path_str, "old")
            .await
            .unwrap();
        ws.write_session_settings(settings_path_str, "new")
            .await
            .unwrap();

        let written = tokio::fs::read_to_string(&settings_path).await.unwrap();
        assert_eq!(written, "new", "settings are rewritten so hook URLs stay current");
    }
}
