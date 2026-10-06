//! Deleting a directory tree the user asked to remove with its contents.

use std::io::ErrorKind;

use super::FsWorkspace;
use crate::error::Error;

impl FsWorkspace {
    /// Delete `path` and everything under it, as
    /// [`delta_usecase::Workspace::remove_dir_tree`] describes: a missing path
    /// is already removed, and a symlink is removed itself, never followed
    /// (`remove_dir_all` does not traverse symlinks).
    pub(super) async fn remove_tree(&self, path: &str) -> Result<(), Error> {
        match tokio::fs::symlink_metadata(path).await {
            Ok(meta) if meta.file_type().is_symlink() => {
                tokio::fs::remove_file(path).await?;
            }
            Ok(_) => tokio::fs::remove_dir_all(path).await?,
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use delta_usecase::Workspace;

    use super::*;

    #[tokio::test]
    async fn removes_a_directory_with_its_contents_and_accepts_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        let tree = dir.path().join("leftover");
        std::fs::create_dir_all(tree.join("nested")).unwrap();
        std::fs::write(tree.join("nested").join("notes.txt"), "work").unwrap();
        let ws = FsWorkspace::new();

        ws.remove_dir_tree(tree.to_str().unwrap()).await.unwrap();
        ws.remove_dir_tree(tree.to_str().unwrap())
            .await
            .expect("a path that is already gone is not an error");

        assert!(!tree.exists());
    }

    #[tokio::test]
    async fn removes_a_symlink_without_following_it() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.txt"), "kept").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        FsWorkspace::new()
            .remove_dir_tree(link.to_str().unwrap())
            .await
            .unwrap();

        assert!(!link.exists());
        assert!(
            target.join("keep.txt").exists(),
            "the link's target is untouched"
        );
    }
}
