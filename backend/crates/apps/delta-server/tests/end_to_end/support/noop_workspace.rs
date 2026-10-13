//! A workspace that touches no filesystem.

use async_trait::async_trait;

use delta_usecase::Workspace;

/// A no-op `Workspace` so `ensure_session` does not touch the real filesystem.
pub(super) struct NoopWorkspace;

#[async_trait]
impl Workspace for NoopWorkspace {
    async fn write_session_settings(
        &self,
        _workdir: &str,
        _settings_json: &str,
    ) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn create_private_dir(&self, _path: &str) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn resolve_existing_dir(&self, path: &str) -> delta_usecase::Result<String> {
        // The end-to-end tests never request a user-selected workdir, so a real
        // resolution is unnecessary; echo the path back as already-canonical.
        Ok(path.to_owned())
    }

    async fn list_dirs(
        &self,
        path: &str,
        _include_hidden: bool,
    ) -> delta_usecase::Result<delta_usecase::DirListing> {
        Ok(delta_usecase::DirListing {
            path: path.to_owned(),
            parent: None,
            entries: Vec::new(),
        })
    }

    async fn remove_dir_tree(&self, _path: &str) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn remove_empty_dir(&self, _path: &str) -> delta_usecase::Result<bool> {
        Ok(false)
    }
}
