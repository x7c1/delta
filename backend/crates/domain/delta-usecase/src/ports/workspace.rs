//! Persisting the Claude Code session settings file and browsing the local
//! filesystem for a session's working directory.

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::dir_listing::DirListing;

/// Persists the settings file that Delta hands to `claude --settings <file>`
/// and exposes the read-only filesystem queries a directory picker needs.
///
/// The session needs a settings file that points Claude Code's HTTP hooks back
/// at this server (and pins the session theme). The server renders that JSON (so
/// the hook URLs always match the running port) and this port writes it to a
/// Delta-owned path, creating parent directories as needed. The path is
/// deliberately *outside* the session's working directory, so spawning or
/// resuming in a real project never clobbers that project's own
/// `.claude/settings.json`.
///
/// A fresh session may also start in a user-selected directory, so this port
/// resolves and browses local directories: [`Self::resolve_existing_dir`]
/// validates a chosen path before launch and [`Self::list_dirs`] backs the
/// picker's browse view.
#[async_trait]
pub trait Workspace: Send + Sync {
    /// Write `settings_json` to `settings_path`, creating parent directories if
    /// absent and overwriting any existing file so the hook URLs always reflect
    /// the current port.
    async fn write_session_settings(&self, settings_path: &str, settings_json: &str) -> Result<()>;

    /// Create the directory `path` (and any missing ancestors) owner-only,
    /// leaving one that already exists as it is.
    ///
    /// Used for the per-spawn scratch directory a session without a repository
    /// is launched in, which nothing else creates: an agent asked to start in a
    /// directory that does not exist would start somewhere else instead (tmux
    /// silently ignores a missing `-c` directory).
    async fn create_private_dir(&self, path: &str) -> Result<()>;

    /// Canonicalize `path` and confirm it is an existing directory, returning
    /// the canonical absolute path.
    ///
    /// Used to validate a user-selected session working directory before launch.
    /// A missing path, a non-directory, or a path that cannot be resolved is an
    /// [`crate::Error::InvalidWorkdir`]. This never creates anything: a missing
    /// directory is rejected, not minted.
    async fn resolve_existing_dir(&self, path: &str) -> Result<String>;

    /// List the immediate subdirectories of `path` for the picker's browse view.
    ///
    /// Returns the canonical `path`, its `parent` (or `None` at a filesystem
    /// root), and the immediate subdirectories sorted by name (case-insensitive),
    /// excluding files. Dot-directories are excluded too unless `include_hidden`
    /// is set. A missing path, a non-directory, or a permission error is an
    /// [`crate::Error::InvalidWorkdir`] / [`crate::Error::WorkdirPermission`]
    /// rather than an opaque I/O failure.
    async fn list_dirs(&self, path: &str, include_hidden: bool) -> Result<DirListing>;

    /// Delete the directory `path` and everything under it.
    ///
    /// Only for a leftover worktree directory git no longer knows, which the
    /// user explicitly asked to remove with its contents: nothing here checks
    /// for work. Symlinks inside the tree are removed, never followed. A path
    /// that does not exist is not an error (there is nothing left to remove).
    async fn remove_dir_tree(&self, path: &str) -> Result<()>;
}

#[async_trait]
impl Workspace for Box<dyn Workspace> {
    async fn write_session_settings(&self, settings_path: &str, settings_json: &str) -> Result<()> {
        (**self)
            .write_session_settings(settings_path, settings_json)
            .await
    }

    async fn create_private_dir(&self, path: &str) -> Result<()> {
        (**self).create_private_dir(path).await
    }

    async fn resolve_existing_dir(&self, path: &str) -> Result<String> {
        (**self).resolve_existing_dir(path).await
    }

    async fn list_dirs(&self, path: &str, include_hidden: bool) -> Result<DirListing> {
        (**self).list_dirs(path, include_hidden).await
    }

    async fn remove_dir_tree(&self, path: &str) -> Result<()> {
        (**self).remove_dir_tree(path).await
    }
}
