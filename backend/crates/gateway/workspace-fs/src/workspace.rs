//! [`FsWorkspace`]: the filesystem-backed [`Workspace`].

use std::path::Path;

use async_trait::async_trait;

use delta_usecase::Workspace;

use crate::error::Error;

/// Prepares the Claude Code session working directory on the local filesystem.
#[derive(Debug, Default, Clone)]
pub struct FsWorkspace;

impl FsWorkspace {
    /// Create a new filesystem workspace gateway.
    pub fn new() -> Self {
        Self
    }

    async fn write(&self, workdir: &str, settings_json: &str) -> Result<(), Error> {
        let claude_dir = Path::new(workdir).join(".claude");
        tokio::fs::create_dir_all(&claude_dir).await?;
        let settings_path = claude_dir.join("settings.json");
        tokio::fs::write(&settings_path, settings_json).await?;
        Ok(())
    }
}

#[async_trait]
impl Workspace for FsWorkspace {
    async fn write_session_settings(
        &self,
        workdir: &str,
        settings_json: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.write(workdir, settings_json)
            .await
            .map_err(delta_usecase::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_settings_creating_the_claude_dir() {
        let dir = tempfile::tempdir().unwrap();
        let workdir = dir.path().join("session");
        let workdir_str = workdir.to_str().unwrap();

        let ws = FsWorkspace::new();
        ws.write_session_settings(workdir_str, r#"{"hooks":{}}"#)
            .await
            .unwrap();

        let written =
            tokio::fs::read_to_string(workdir.join(".claude").join("settings.json"))
                .await
                .unwrap();
        assert_eq!(written, r#"{"hooks":{}}"#);
    }

    #[tokio::test]
    async fn overwrites_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let workdir_str = dir.path().to_str().unwrap();
        let ws = FsWorkspace::new();

        ws.write_session_settings(workdir_str, "old").await.unwrap();
        ws.write_session_settings(workdir_str, "new").await.unwrap();

        let written =
            tokio::fs::read_to_string(dir.path().join(".claude").join("settings.json"))
                .await
                .unwrap();
        assert_eq!(written, "new", "settings are rewritten so hook URLs stay current");
    }
}
