use std::path::PathBuf;

/// The app log directory for `identifier`, as Tauri's path resolver names it
/// with `app_log_dir()`: `~/Library/Logs/<identifier>` on macOS,
/// `<platform local data dir>/<identifier>/logs` elsewhere. `None` without a
/// home directory to resolve it from.
///
/// Worked out here rather than asked of the resolver because logging starts
/// before Tauri does; `super::warn_if_the_resolver_disagrees` checks the two
/// agree once the app is up.
pub fn app_log_dir(identifier: &str) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|home| home.join("Library/Logs").join(identifier))
    } else {
        dirs::data_local_dir().map(|dir| dir.join(identifier).join("logs"))
    }
}
