use tauri::{AppHandle, Manager};

use super::LogGuard;

/// Warn when the directory the log is written in is not the one Tauri's path
/// resolver names as the app log directory — the one an erase removes.
pub fn warn_if_the_resolver_disagrees(app: &AppHandle, guard: &LogGuard) {
    let Some(dir) = &guard.dir else {
        return;
    };
    match app.path().app_log_dir() {
        Ok(resolved) if &resolved == dir => {}
        Ok(resolved) => tracing::warn!(
            log_dir = %dir.display(),
            app_log_dir = %resolved.display(),
            "the log is written outside the app log directory, so an erase leaves it behind"
        ),
        Err(err) => tracing::warn!("could not resolve the app log directory: {err}"),
    }
}
