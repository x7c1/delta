//! Finishing an erase in the desktop app.
//!
//! The server's erase removes what Delta created and its own data directory;
//! what the shell itself left under the app's bundle identifier — the webview's
//! storage and caches, and any file the shell writes under the per-app
//! directories — is the shell's to remove. WebKit writes those back while the
//! webview is alive, so they are removed only as the process ends
//! ([`EraseExit`] marks such an exit; [`remove_app_dirs`] does the removal).

use std::fmt::Display;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};

/// Managed state: whether the app is quitting because the server erased
/// everything, so the per-app directories go when the process ends.
#[derive(Default)]
pub struct EraseExit(AtomicBool);

impl EraseExit {
    /// Record that this exit is an erase.
    pub fn mark(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether [`mark`](Self::mark) was called.
    pub fn is_marked(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The platforms the desktop app is built for, as far as where the shell's
/// files live is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// WKWebView keeps its storage outside the app data directory, in
    /// `~/Library/WebKit/<identifier>`.
    MacOs,
    /// WebKitGTK keeps its storage inside the app data directory.
    Linux,
}

impl Platform {
    /// The platform this build runs on.
    pub const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::MacOs
    } else {
        Self::Linux
    };
}

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

/// The directories an erase removes as the process ends: every per-app
/// directory the resolver names, and on macOS the webview's storage
/// (`~/Library/WebKit/<identifier>`), which the resolver does not name. (The
/// webview's caches are in the app cache directory on macOS and in the app
/// data directory on Linux.)
///
/// The same directory is listed once, and one inside another listed directory
/// not at all. A path without the identifier as one of its components is left
/// out, so a resolver gone wrong cannot point the removal at a shared
/// directory.
pub fn erase_paths(identifier: &str, dirs: &ResolverDirs, platform: Platform) -> Vec<PathBuf> {
    let webkit = match platform {
        Platform::MacOs => dirs
            .home
            .as_ref()
            .map(|home| home.join("Library/WebKit").join(identifier)),
        Platform::Linux => None,
    };
    let candidates: Vec<PathBuf> = [
        &dirs.app_data,
        &dirs.app_local_data,
        &dirs.app_config,
        &dirs.app_cache,
        &dirs.app_log,
        &webkit,
    ]
    .into_iter()
    .flatten()
    .filter(|path| names_identifier(path, identifier))
    .cloned()
    .collect();

    let mut paths: Vec<PathBuf> = Vec::new();
    for (i, path) in candidates.iter().enumerate() {
        let covered = candidates
            .iter()
            .enumerate()
            .any(|(j, other)| path.starts_with(other) && (path != other || j < i));
        if !covered {
            paths.push(path.clone());
        }
    }
    paths
}

/// Whether `identifier` is one of `path`'s components.
fn names_identifier(path: &Path, identifier: &str) -> bool {
    !identifier.is_empty()
        && path
            .components()
            .any(|component| component == Component::Normal(identifier.as_ref()))
}

/// Remove the directories [`erase_paths`] lists for this app. A missing one is
/// not an error; any other failure is logged and the rest are still removed.
pub fn remove_app_dirs(app: &AppHandle) {
    let identifier = &app.config().identifier;
    let dirs = ResolverDirs::resolve(app);
    for path in erase_paths(identifier, &dirs, Platform::CURRENT) {
        match std::fs::remove_dir_all(&path) {
            Ok(()) => tracing::info!(path = %path.display(), "removed the app's directory"),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(path = %path.display(), "could not remove the app's directory: {err}");
            }
        }
    }
}

/// The message the shell shows once the server has erased everything: that
/// Delta will quit, then each kept item with why it was kept.
pub fn report_message<T: Display>(kept: impl IntoIterator<Item = T>) -> String {
    let kept: Vec<String> = kept.into_iter().map(|item| format!("- {item}")).collect();
    let mut message = String::from("Delta removed its files and will quit.\n\n");
    if kept.is_empty() {
        message.push_str("Nothing was kept.");
    } else {
        message.push_str("Kept:\n");
        message.push_str(&kept.join("\n"));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "io.github.x7c1.delta";

    /// What Tauri's resolver names on macOS for [`ID`].
    fn macos_dirs() -> ResolverDirs {
        let support = PathBuf::from("/Users/u/Library/Application Support").join(ID);
        ResolverDirs {
            app_data: Some(support.clone()),
            app_local_data: Some(support.clone()),
            app_config: Some(support),
            app_cache: Some(PathBuf::from("/Users/u/Library/Caches").join(ID)),
            app_log: Some(PathBuf::from("/Users/u/Library/Logs").join(ID)),
            home: Some(PathBuf::from("/Users/u")),
        }
    }

    /// What Tauri's resolver names on Linux for [`ID`].
    fn linux_dirs() -> ResolverDirs {
        let data = PathBuf::from("/home/u/.local/share").join(ID);
        ResolverDirs {
            app_data: Some(data.clone()),
            app_local_data: Some(data.clone()),
            app_config: Some(PathBuf::from("/home/u/.config").join(ID)),
            app_cache: Some(PathBuf::from("/home/u/.cache").join(ID)),
            app_log: Some(data.join("logs")),
            home: Some(PathBuf::from("/home/u")),
        }
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn on_macos_the_support_caches_logs_and_webkit_directories_go() {
        assert_eq!(
            erase_paths(ID, &macos_dirs(), Platform::MacOs),
            paths(&[
                "/Users/u/Library/Application Support/io.github.x7c1.delta",
                "/Users/u/Library/Caches/io.github.x7c1.delta",
                "/Users/u/Library/Logs/io.github.x7c1.delta",
                "/Users/u/Library/WebKit/io.github.x7c1.delta",
            ])
        );
    }

    #[test]
    fn on_linux_the_data_config_and_cache_directories_go() {
        // The webview's storage and the log directory are inside the data
        // directory.
        assert_eq!(
            erase_paths(ID, &linux_dirs(), Platform::Linux),
            paths(&[
                "/home/u/.local/share/io.github.x7c1.delta",
                "/home/u/.config/io.github.x7c1.delta",
                "/home/u/.cache/io.github.x7c1.delta",
            ])
        );
    }

    #[test]
    fn a_directory_the_resolver_could_not_name_is_left_out() {
        let dirs = ResolverDirs {
            app_cache: None,
            home: None,
            ..macos_dirs()
        };
        assert_eq!(
            erase_paths(ID, &dirs, Platform::MacOs),
            paths(&[
                "/Users/u/Library/Application Support/io.github.x7c1.delta",
                "/Users/u/Library/Logs/io.github.x7c1.delta",
            ])
        );
    }

    #[test]
    fn a_path_without_the_identifier_is_never_removed() {
        let dirs = ResolverDirs {
            app_config: Some(PathBuf::from("/home/u/.config")),
            ..linux_dirs()
        };
        assert!(
            !erase_paths(ID, &dirs, Platform::Linux).contains(&PathBuf::from("/home/u/.config"))
        );
        // A component that only starts with the identifier does not name it.
        let dirs = ResolverDirs {
            app_cache: Some(PathBuf::from("/home/u/.cache/io.github.x7c1.delta.dev")),
            ..linux_dirs()
        };
        assert!(!erase_paths(ID, &dirs, Platform::Linux)
            .contains(&PathBuf::from("/home/u/.cache/io.github.x7c1.delta.dev")));
        assert!(erase_paths("", &linux_dirs(), Platform::Linux).is_empty());
    }

    #[test]
    fn the_report_lists_each_kept_item() {
        assert_eq!(
            report_message([
                "worktree /w/a (it has modified or untracked files)",
                "branch feature (it is not merged)"
            ]),
            "Delta removed its files and will quit.\n\n\
             Kept:\n\
             - worktree /w/a (it has modified or untracked files)\n\
             - branch feature (it is not merged)"
        );
    }

    #[test]
    fn the_report_says_when_nothing_was_kept() {
        assert_eq!(
            report_message(Vec::<String>::new()),
            "Delta removed its files and will quit.\n\nNothing was kept."
        );
    }
}
