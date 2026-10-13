//! The per-app directories to erase, as Tauri's path resolver names them.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// The per-app directories Tauri's path resolver names for this identifier,
/// and the home directory; `None` where the resolver could not name one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolverDirs {
    pub app_data: Option<PathBuf>,
    pub app_local_data: Option<PathBuf>,
    pub app_config: Option<PathBuf>,
    pub app_cache: Option<PathBuf>,
    pub app_log: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl ResolverDirs {
    /// Ask the app's path resolver; a directory it cannot name is logged and
    /// left out.
    pub fn resolve(app: &AppHandle) -> Self {
        let path = app.path();
        let named = |what: &str, result: tauri::Result<PathBuf>| match result {
            Ok(dir) => Some(dir),
            Err(err) => {
                tracing::warn!("could not resolve the {what} directory to erase: {err}");
                None
            }
        };
        Self {
            app_data: named("app data", path.app_data_dir()),
            app_local_data: named("app local data", path.app_local_data_dir()),
            app_config: named("app config", path.app_config_dir()),
            app_cache: named("app cache", path.app_cache_dir()),
            app_log: named("app log", path.app_log_dir()),
            home: named("home", path.home_dir()),
        }
    }
}
