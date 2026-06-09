//! [`FsWorkspace`]: the filesystem-backed [`Workspace`].

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use delta_usecase::{DirEntry, DirListing, Workspace};

use crate::error::Error;

/// Writes the Claude Code session settings file and browses local directories.
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

    /// Canonicalize `path` and confirm it is an existing directory.
    ///
    /// Canonicalization both resolves `.`/`..`/symlinks and fails on a missing
    /// path, so it doubles as the existence check; a successful canonical path
    /// that is not a directory is rejected too. Never creates anything.
    async fn resolve_dir(&self, path: &str) -> Result<PathBuf, Error> {
        let canonical = tokio::fs::canonicalize(path).await.map_err(|err| {
            map_dir_error(path, err, "no such directory or it could not be resolved")
        })?;
        let meta = tokio::fs::metadata(&canonical)
            .await
            .map_err(|err| map_dir_error(path, err, "could not stat the path"))?;
        if !meta.is_dir() {
            return Err(Error::InvalidWorkdir(format!("{path}: not a directory")));
        }
        Ok(canonical)
    }

    async fn list(&self, path: &str) -> Result<DirListing, Error> {
        let dir = self.resolve_dir(path).await?;

        let mut read = tokio::fs::read_dir(&dir)
            .await
            .map_err(|err| map_dir_error(path, err, "could not read the directory"))?;

        let mut entries = Vec::new();
        while let Some(entry) = read
            .next_entry()
            .await
            .map_err(|err| map_dir_error(path, err, "could not read a directory entry"))?
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Skip dot-directories by default; the picker hides them.
            if name.starts_with('.') {
                continue;
            }
            // Only directories are browseable targets. `file_type` avoids a
            // symlink-following `metadata` call; a symlinked directory still
            // reports `is_symlink`, so confirm via `metadata` only then.
            let file_type = entry
                .file_type()
                .await
                .map_err(|err| map_dir_error(path, err, "could not read an entry's type"))?;
            let is_dir = if file_type.is_symlink() {
                // Follow the link to see whether it points at a directory.
                match tokio::fs::metadata(entry.path()).await {
                    Ok(meta) => meta.is_dir(),
                    // A broken symlink is simply not a browseable directory.
                    Err(_) => false,
                }
            } else {
                file_type.is_dir()
            };
            if !is_dir {
                continue;
            }
            entries.push(DirEntry {
                name,
                path: entry.path().to_string_lossy().into_owned(),
            });
        }

        // Case-insensitive name sort for a stable, human-friendly order.
        entries.sort_by_key(|e| e.name.to_lowercase());

        let parent = dir.parent().map(|p| p.to_string_lossy().into_owned());

        Ok(DirListing {
            path: dir.to_string_lossy().into_owned(),
            parent,
            entries,
        })
    }
}

/// Map a filesystem I/O error against a browsed path onto the right
/// gateway-level [`Error`]: a permission failure is its own variant (mapped to
/// `403`), everything else is an invalid working directory (`400`). `context`
/// is a short human note for the non-permission case.
fn map_dir_error(path: &str, err: std::io::Error, context: &str) -> Error {
    match err.kind() {
        ErrorKind::PermissionDenied => Error::Permission(format!("{path}: permission denied")),
        _ => Error::InvalidWorkdir(format!("{path}: {context}")),
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
    ) -> std::result::Result<DirListing, delta_usecase::Error> {
        Ok(self.list(path).await?)
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

    #[tokio::test]
    async fn resolve_existing_dir_returns_the_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let ws = FsWorkspace::new();

        let resolved = ws
            .resolve_existing_dir(dir.path().to_str().unwrap())
            .await
            .unwrap();

        // The canonical form of the temp dir (macOS resolves `/var` →
        // `/private/var`, so compare against the canonicalized expectation).
        let expected = tokio::fs::canonicalize(dir.path())
            .await
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(resolved, expected);
    }

    #[tokio::test]
    async fn resolve_existing_dir_rejects_a_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let ws = FsWorkspace::new();

        let err = ws
            .resolve_existing_dir(missing.to_str().unwrap())
            .await
            .unwrap_err();
        assert!(
            matches!(err, delta_usecase::Error::InvalidWorkdir(_)),
            "a missing path is an InvalidWorkdir, got {err:?}"
        );
    }

    #[tokio::test]
    async fn resolve_existing_dir_rejects_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a-file");
        tokio::fs::write(&file, "x").await.unwrap();
        let ws = FsWorkspace::new();

        let err = ws
            .resolve_existing_dir(file.to_str().unwrap())
            .await
            .unwrap_err();
        assert!(
            matches!(err, delta_usecase::Error::InvalidWorkdir(_)),
            "a regular file is not a directory, got {err:?}"
        );
    }

    #[tokio::test]
    async fn list_dirs_returns_sorted_subdirs_only_hiding_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Two subdirectories (out of alphabetical order on disk), a dot-dir, and
        // a regular file. Only the two visible subdirectories should appear.
        tokio::fs::create_dir(root.join("Zebra")).await.unwrap();
        tokio::fs::create_dir(root.join("alpha")).await.unwrap();
        tokio::fs::create_dir(root.join(".hidden")).await.unwrap();
        tokio::fs::write(root.join("file.txt"), "x").await.unwrap();

        let ws = FsWorkspace::new();
        let listing = ws.list_dirs(root.to_str().unwrap()).await.unwrap();

        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["alpha", "Zebra"],
            "dirs only, dotfiles hidden, case-insensitive name order"
        );
        // The canonical path round-trips and `parent` is populated below a root.
        let canonical_root = tokio::fs::canonicalize(root).await.unwrap();
        assert_eq!(listing.path, canonical_root.to_string_lossy());
        assert_eq!(
            listing.parent.as_deref(),
            canonical_root.parent().map(|p| p.to_str().unwrap()),
            "parent points one level up"
        );
        // Each entry's path is absolute and under the listed directory.
        for entry in &listing.entries {
            assert!(entry.path.starts_with(canonical_root.to_str().unwrap()));
        }
    }

    #[tokio::test]
    async fn list_dirs_rejects_a_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        let ws = FsWorkspace::new();

        let err = ws
            .list_dirs(missing.to_str().unwrap())
            .await
            .unwrap_err();
        assert!(
            matches!(err, delta_usecase::Error::InvalidWorkdir(_)),
            "a missing path is an InvalidWorkdir, got {err:?}"
        );
    }

    #[tokio::test]
    async fn list_dirs_reports_root_with_no_parent() {
        let ws = FsWorkspace::new();
        let listing = ws.list_dirs("/").await.unwrap();
        assert_eq!(listing.path, "/");
        assert!(
            listing.parent.is_none(),
            "the filesystem root has no parent"
        );
    }
}
