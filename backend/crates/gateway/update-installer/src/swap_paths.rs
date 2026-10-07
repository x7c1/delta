use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

/// The name the new bundle is copied to, next to the running one, before
/// it takes the running one's place: hidden, and without `.app`, so that
/// neither Finder nor Launch Services shows it as an app.
pub const STAGING_NAME: &str = ".delta-update-new";

/// The name the running bundle is moved to, next to where it was, when the
/// new one takes its place. Removed at the next launch
/// ([`remove_leftovers`]).
pub const BACKUP_NAME: &str = ".delta-update-backup";

/// Where an update of the bundle at `bundle` is staged and where the bundle
/// it replaces is kept: [`STAGING_NAME`] and [`BACKUP_NAME`] in the bundle's
/// directory, so that both moves are renames on one volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapPaths {
    /// The directory the bundle is in, where both moves happen.
    pub dir: PathBuf,
    pub bundle: PathBuf,
    pub staging: PathBuf,
    pub backup: PathBuf,
}

impl SwapPaths {
    /// The paths of an update of `bundle`; `None` for a path with no parent.
    pub fn of(bundle: &Path) -> Option<Self> {
        let dir = bundle.parent()?;
        Some(Self {
            dir: dir.to_path_buf(),
            bundle: bundle.to_path_buf(),
            staging: dir.join(STAGING_NAME),
            backup: dir.join(BACKUP_NAME),
        })
    }

    /// Put the bundle staged at [`Self::staging`] in place of
    /// [`Self::bundle`]: move the bundle to [`Self::backup`], then the staged
    /// one to the bundle's path. If the second move fails, the first is
    /// undone, so the bundle is never left missing. The running app's files
    /// are never written to: it keeps running from the backup until it
    /// exits.
    ///
    /// `Err` is the one-line reason the update could not be put in place.
    pub fn swap(&self) -> Result<(), String> {
        let Self {
            bundle,
            staging,
            backup,
            ..
        } = self;
        remove_any(backup)
            .map_err(|err| format!("could not remove {}: {err}", backup.display()))?;
        std::fs::rename(bundle, backup).map_err(|err| {
            format!(
                "could not move {} aside to {}: {err}",
                bundle.display(),
                backup.display()
            )
        })?;
        let Err(err) = std::fs::rename(staging, bundle) else {
            return Ok(());
        };
        let moved = format!(
            "could not move the new app from {} to {}: {err}",
            staging.display(),
            bundle.display()
        );
        match std::fs::rename(backup, bundle) {
            Ok(()) => Err(format!("{moved}; the current app is left as it was")),
            Err(restore) => Err(format!(
                "{moved}, nor move the current app back from {}: {restore}",
                backup.display()
            )),
        }
    }

    /// Remove what an update of this bundle may have left next to it: the
    /// backup of the bundle it replaced, and a staged copy it never put in
    /// place. Each one that cannot be removed is logged.
    pub fn remove_leftovers(&self) {
        for leftover in [&self.backup, &self.staging] {
            match remove_any(leftover) {
                Ok(true) => tracing::info!(
                    path = %leftover.display(),
                    "removed what the last update of Delta left next to the app"
                ),
                Ok(false) => {}
                Err(err) => tracing::warn!(
                    path = %leftover.display(),
                    error = %err,
                    "could not remove what the last update of Delta left next to the app"
                ),
            }
        }
    }
}

/// Remove whatever is at `path` — a directory with all it holds, a file or
/// a symlink (never what it points to). `Ok(false)` when nothing was there.
pub fn remove_any(path: &Path) -> io::Result<bool> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_version(bundle: &Path) -> String {
        std::fs::read_to_string(bundle.join("Contents/MacOS/delta-desktop")).unwrap()
    }

    fn write_bundle(bundle: &Path, version: &str) {
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(bundle.join("Contents/MacOS/delta-desktop"), version).unwrap();
    }

    #[test]
    fn a_swap_puts_the_new_bundle_in_place_and_keeps_the_old_as_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = SwapPaths::of(&dir.path().join("Delta.app")).unwrap();
        assert_eq!(paths.staging, dir.path().join(".delta-update-new"));
        assert_eq!(paths.backup, dir.path().join(".delta-update-backup"));
        write_bundle(&paths.bundle, "0.5.0");
        write_bundle(&paths.staging, "0.6.0");
        // A backup an earlier update left behind is replaced.
        write_bundle(&paths.backup, "0.4.0");

        paths.swap().unwrap();
        assert_eq!(bundle_version(&paths.bundle), "0.6.0");
        assert_eq!(bundle_version(&paths.backup), "0.5.0");
        assert!(!paths.staging.exists());
    }

    #[test]
    fn a_failed_second_move_restores_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let paths = SwapPaths::of(&dir.path().join("Delta.app")).unwrap();
        write_bundle(&paths.bundle, "0.5.0");
        // Nothing staged: moving it into place fails.
        let reason = paths.swap().unwrap_err();
        assert!(
            reason.contains("could not move the new app")
                && reason.ends_with("the current app is left as it was"),
            "{reason}"
        );
        assert_eq!(bundle_version(&paths.bundle), "0.5.0");
        assert!(!paths.backup.exists());
    }

    #[test]
    fn a_missing_bundle_is_not_swapped() {
        let dir = tempfile::tempdir().unwrap();
        let paths = SwapPaths::of(&dir.path().join("Delta.app")).unwrap();
        write_bundle(&paths.staging, "0.6.0");
        let reason = paths.swap().unwrap_err();
        assert!(reason.starts_with("could not move"), "{reason}");
        assert_eq!(bundle_version(&paths.staging), "0.6.0");
        assert!(!paths.bundle.exists());
    }

    #[test]
    fn the_leftovers_of_an_update_are_removed_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let paths = SwapPaths::of(&dir.path().join("Delta.app")).unwrap();
        write_bundle(&paths.bundle, "0.6.0");
        write_bundle(&paths.backup, "0.5.0");
        write_bundle(&paths.staging, "0.6.0");
        let other = dir.path().join("Other.app");
        write_bundle(&other, "1.0.0");

        paths.remove_leftovers();
        assert!(!paths.backup.exists());
        assert!(!paths.staging.exists());
        assert_eq!(bundle_version(&paths.bundle), "0.6.0");
        assert_eq!(bundle_version(&other), "1.0.0");
        // Nothing left: nothing to do.
        paths.remove_leftovers();
    }

    #[test]
    fn a_symlink_is_removed_not_what_it_points_to() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        write_bundle(&target, "0.5.0");
        let link = dir.path().join(BACKUP_NAME);
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(remove_any(&link).unwrap());
        assert_eq!(bundle_version(&target), "0.5.0");
        assert!(!remove_any(&link).unwrap());
    }
}
