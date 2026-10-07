//! Removing downloaded updates the running app has caught up with.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use semver::Version;

/// Remove the files in the update directory `dir` whose release is not newer
/// than `running`, the version of the app now running, and return what was
/// removed.
///
/// Run at startup, so an update that has been installed (and the app
/// restarted into) does not linger in the data directory. A file's release is
/// read from its name, by the release workflow's asset naming
/// (`delta-desktop_<version>_amd64.deb`, `Delta_<version>_aarch64.dmg`), and
/// a file named for a newer release is kept, ready or not. A file whose name
/// names no version is left alone: the next download clears the directory.
/// A missing directory removes nothing; a file that cannot be removed is
/// logged and kept.
pub fn remove_stale_updates(dir: &Path, running: &Version) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            tracing::warn!(dir = %dir.display(), error = %err, "could not list the downloaded updates");
            return Vec::new();
        }
    };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(version) = name.to_str().and_then(release_of) else {
            continue;
        };
        if version > *running {
            continue;
        }
        let path = entry.path();
        match std::fs::remove_file(&path) {
            Ok(()) => {
                tracing::info!(
                    path = %path.display(),
                    version = %version,
                    "removed a downloaded update the running app is not older than"
                );
                removed.push(path);
            }
            Err(err) => tracing::warn!(
                path = %path.display(),
                error = %err,
                "could not remove a downloaded update the running app is not older than"
            ),
        }
    }
    removed
}

/// The release an update file's name names: the field between its first two
/// `_`s.
fn release_of(name: &str) -> Option<Version> {
    Version::parse(name.split('_').nth(1)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_of_a_release_not_newer_than_the_running_one_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let names = [
            "delta-desktop_0.4.9_amd64.deb",
            "delta-desktop_0.5.0_amd64.deb",
            "delta-desktop_0.5.0_amd64.deb.part",
            "Delta_0.5.0_aarch64.dmg",
            "delta-desktop_0.6.0_amd64.deb",
            "delta-desktop_0.6.0_amd64.deb.part",
            "notes.txt",
        ];
        for name in names {
            std::fs::write(dir.path().join(name), name).unwrap();
        }

        let mut removed = remove_stale_updates(dir.path(), &Version::new(0, 5, 0));
        removed.sort();
        let mut expected: Vec<PathBuf> = [
            "Delta_0.5.0_aarch64.dmg",
            "delta-desktop_0.4.9_amd64.deb",
            "delta-desktop_0.5.0_amd64.deb",
            "delta-desktop_0.5.0_amd64.deb.part",
        ]
        .iter()
        .map(|name| dir.path().join(name))
        .collect();
        expected.sort();
        assert_eq!(removed, expected);

        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "delta-desktop_0.6.0_amd64.deb",
                "delta-desktop_0.6.0_amd64.deb.part",
                "notes.txt",
            ]
        );
    }

    #[test]
    fn a_missing_directory_removes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            remove_stale_updates(&dir.path().join("updates"), &Version::new(0, 5, 0)).is_empty()
        );
    }
}
