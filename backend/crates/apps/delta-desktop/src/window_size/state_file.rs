use std::path::PathBuf;

use tauri::{AppHandle, Manager, Runtime};

use super::STATE_FILE_NAME;

/// The state file's path, or `None` when the app config directory cannot be
/// resolved.
pub fn state_file<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    match app.path().app_config_dir() {
        Ok(dir) => Some(dir.join(STATE_FILE_NAME)),
        Err(err) => {
            tracing::warn!("could not resolve the window size file: {err}");
            None
        }
    }
}
