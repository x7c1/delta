//! The fixed directory the helper works in, which only root may change.

use std::fs::{DirBuilder, Metadata};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::Path;

use crate::Refusal;

/// Where the helper stages the copy it checks and installs: a directory of
/// its own under `/var/cache`, the place for files a package's programs
/// keep only while they need them.
///
/// Fixed rather than taken from `TMPDIR` (`std::env::temp_dir`): the helper
/// runs as root, and must not let its caller's environment choose where
/// root writes.
pub const WORK_DIR: &str = "/var/cache/delta-update-helper";

/// The permission bits that let the group or others change a directory's
/// entries.
const WRITABLE_BY_OTHERS: u32 = 0o022;

/// Make `dir` ready to work in: created (mode 0700) when missing, and only
/// `owner` (root, in the helper) able to change it or its parent.
///
/// Refused ([`Refusal::WorkDir`]) when the parent, or `dir` itself, is not a
/// directory `owner` owns, or the group or others may write to it: anyone
/// else who could write there could swap the staged copy between the checks
/// and the install. A symlink at `dir` is refused too.
pub fn prepare(dir: &Path, owner: u32) -> Result<(), Refusal> {
    let refuse = |reason: String| Refusal::WorkDir {
        dir: dir.to_path_buf(),
        reason,
    };
    let parent = dir
        .parent()
        .ok_or_else(|| refuse("it has no parent directory".to_owned()))?;
    let metadata = std::fs::metadata(parent).map_err(|source| Refusal::Io {
        what: format!("could not read {}", parent.display()),
        source,
    })?;
    if let Some(reason) = exposed(&metadata, owner) {
        return Err(refuse(format!("its parent {}: {reason}", parent.display())));
    }

    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(source) => {
            return Err(Refusal::Io {
                what: format!("could not create {}", dir.display()),
                source,
            })
        }
    }
    let metadata = std::fs::symlink_metadata(dir).map_err(|source| Refusal::Io {
        what: format!("could not read {}", dir.display()),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(refuse("it is a symlink".to_owned()));
    }
    match exposed(&metadata, owner) {
        Some(reason) => Err(refuse(reason)),
        None => Ok(()),
    }
}

/// Why a directory with `metadata` is not one only `owner` may change, if
/// it is not.
fn exposed(metadata: &Metadata, owner: u32) -> Option<String> {
    if !metadata.is_dir() {
        Some("it is not a directory".to_owned())
    } else if metadata.uid() != owner {
        Some(format!(
            "it is owned by uid {}, not {owner}",
            metadata.uid()
        ))
    } else if metadata.mode() & WRITABLE_BY_OTHERS != 0 {
        Some(format!(
            "its group or others may write to it (mode {:o})",
            metadata.mode() & 0o7777
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{symlink, PermissionsExt};

    use super::*;
    use crate::child_env::{in_child, run_in_child};
    use crate::staged_copy::StagedCopy;

    fn me() -> u32 {
        // SAFETY: `geteuid` has no preconditions and cannot fail.
        unsafe { libc::geteuid() }
    }

    fn chmod(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A fresh directory only its owner may change, whatever the process
    /// umask: `tempfile` creates it through the umask, and under umask 002
    /// it would come out mode 775 and be refused as a parent.
    fn private_tempdir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        chmod(dir.path(), 0o700);
        dir
    }

    fn reason(result: Result<(), Refusal>) -> String {
        match result {
            Err(Refusal::WorkDir { reason, .. }) => reason,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_fixed_directory_is_absolute_and_not_temporary() {
        let dir = Path::new(WORK_DIR);
        assert!(dir.is_absolute());
        assert_eq!(dir.parent(), Some(Path::new("/var/cache")));
    }

    #[test]
    fn a_missing_directory_is_created_owner_only_and_an_existing_one_kept() {
        let parent = tempfile::tempdir().unwrap();
        chmod(parent.path(), 0o755);
        let dir = parent.path().join("work");

        prepare(&dir, me()).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        std::fs::write(dir.join("kept"), "x").unwrap();
        prepare(&dir, me()).unwrap();
        assert!(dir.join("kept").exists());
    }

    #[test]
    fn a_parent_others_may_write_to_is_refused_before_anything_is_created() {
        for mode in [0o775, 0o757, 0o777, 0o1777] {
            let parent = tempfile::tempdir().unwrap();
            chmod(parent.path(), mode);
            let dir = parent.path().join("work");

            let reason = reason(prepare(&dir, me()));
            assert!(reason.starts_with("its parent "), "{mode:o}: {reason}");
            assert!(reason.contains("may write to it"), "{mode:o}: {reason}");
            assert!(!dir.exists(), "{mode:o}");
            chmod(parent.path(), 0o700);
        }
    }

    #[test]
    fn a_directory_others_may_write_to_is_refused() {
        for mode in [0o720, 0o702, 0o777] {
            let parent = private_tempdir();
            let dir = parent.path().join("work");
            std::fs::create_dir(&dir).unwrap();
            chmod(&dir, mode);

            let reason = reason(prepare(&dir, me()));
            assert!(!reason.starts_with("its parent "), "{mode:o}: {reason}");
            assert!(reason.contains("may write to it"), "{mode:o}: {reason}");
        }
    }

    #[test]
    fn a_directory_owned_by_someone_else_is_refused() {
        let parent = private_tempdir();
        let reason = reason(prepare(&parent.path().join("work"), me() + 1));
        assert!(reason.contains("owned by uid"), "{reason}");
    }

    #[test]
    fn a_symlink_or_a_file_is_refused() {
        let parent = private_tempdir();
        let elsewhere = tempfile::tempdir().unwrap();
        let link = parent.path().join("link");
        symlink(elsewhere.path(), &link).unwrap();
        assert_eq!(reason(prepare(&link, me())), "it is a symlink");

        let file = parent.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(reason(prepare(&file, me())), "it is not a directory");
    }

    /// The directory, outside `TMPDIR`, the child of
    /// [`staging_ignores_tmpdir`] works in.
    const ROOT_VAR: &str = "DELTA_UPDATE_HELPER_TEST_ROOT";

    /// The helper stages its copy in the directory it is given, never in
    /// `TMPDIR`, whatever `TMPDIR` says.
    #[test]
    fn staging_ignores_tmpdir() {
        let tmpdir = tempfile::tempdir().unwrap();
        let root = private_tempdir();
        run_in_child(
            "work_dir::tests::child_staging_ignores_tmpdir",
            &[
                ("TMPDIR", tmpdir.path().as_os_str()),
                (ROOT_VAR, root.path().as_os_str()),
            ],
        );
        assert_eq!(std::fs::read_dir(tmpdir.path()).unwrap().count(), 0);
    }

    #[test]
    #[ignore = "run by staging_ignores_tmpdir in a child process"]
    fn child_staging_ignores_tmpdir() {
        if !in_child() {
            return;
        }
        let tmpdir = std::env::var_os("TMPDIR").expect("the parent sets TMPDIR");
        let root =
            std::path::PathBuf::from(std::env::var_os(ROOT_VAR).expect("the parent sets it"));
        let source = root.join("delta-desktop_0.6.0_amd64.deb");
        std::fs::write(&source, "hello\n").unwrap();
        let work = root.join("work");

        prepare(&work, me()).unwrap();
        let staged = StagedCopy::copy(&source, &work).unwrap();
        assert!(staged.path().starts_with(&work));
        assert!(!staged.path().starts_with(&tmpdir));
        assert_eq!(std::fs::read_dir(&tmpdir).unwrap().count(), 0);
    }
}
