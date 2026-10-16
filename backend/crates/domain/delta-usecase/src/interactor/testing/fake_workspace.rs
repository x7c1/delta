//! In-memory [`Workspace`] fake recording settings writes and modelling a small
//! set of "existing" directories for the workdir-validation path.

use std::sync::Mutex;
use std::time::SystemTime;

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::Workspace;

use super::TEST_WORKDIR;

/// Records the session settings written, so tests can assert the path and the
/// rendered JSON the server passed in. Also models a small set of "existing"
/// directories so the workdir-validation path can be exercised: a resolvable
/// path is returned canonicalized (here, prefixed with `/canon` so a test can
/// tell the canonical form apart from the input), anything else is an
/// `InvalidWorkdir`.
pub(crate) struct FakeWorkspace {
    pub(crate) written: Mutex<Vec<(String, String)>>,
    /// Paths that "exist" as directories; `resolve_existing_dir` accepts these.
    /// Starts with [`TEST_WORKDIR`], the directory tests that need no
    /// particular one start their sessions in.
    pub(crate) existing_dirs: Mutex<Vec<String>>,
    /// Scripted `list_dirs` children, keyed by the listed path: the bare names
    /// of its subdirectories. A path absent from the map lists no children.
    pub(crate) children: Mutex<Vec<(String, Vec<String>)>>,
    /// Scripted `modified_at` answers, keyed by path. A path absent from it
    /// fails, as a directory whose metadata cannot be read does.
    pub(crate) modified: Mutex<Vec<(String, SystemTime)>>,
    /// The paths `remove_dir_tree` was asked to delete, in order.
    pub(crate) removed_trees: Mutex<Vec<String>>,
    /// Directories `remove_empty_dir` finds non-empty (it leaves them).
    pub(crate) non_empty_dirs: Mutex<Vec<String>>,
    /// The directories `remove_empty_dir` removed, in order.
    pub(crate) removed_empty_dirs: Mutex<Vec<String>>,
}

impl Default for FakeWorkspace {
    fn default() -> Self {
        Self {
            written: Mutex::default(),
            existing_dirs: Mutex::new(vec![TEST_WORKDIR.to_owned()]),
            children: Mutex::default(),
            modified: Mutex::default(),
            removed_trees: Mutex::default(),
            non_empty_dirs: Mutex::default(),
            removed_empty_dirs: Mutex::default(),
        }
    }
}

impl FakeWorkspace {
    /// The canonical form this fake assigns to a resolvable directory, so tests
    /// can assert the *canonical* path (not the raw input) reaches the launch.
    pub(crate) fn canonical(path: &str) -> String {
        format!("/canon{path}")
    }

    /// Script `list_dirs(dir)` to list subdirectories named `names`.
    pub(crate) fn with_children(self, dir: &str, names: &[&str]) -> Self {
        self.children.lock().unwrap().push((
            dir.to_owned(),
            names.iter().map(|name| (*name).to_owned()).collect(),
        ));
        self
    }

    /// Script `modified_at(path)` to report `at`.
    pub(crate) fn with_modified(self, path: &str, at: SystemTime) -> Self {
        self.modified.lock().unwrap().push((path.to_owned(), at));
        self
    }
}

#[async_trait]
impl Workspace for FakeWorkspace {
    async fn write_session_settings(&self, settings_path: &str, settings_json: &str) -> Result<()> {
        self.written
            .lock()
            .unwrap()
            .push((settings_path.to_owned(), settings_json.to_owned()));
        Ok(())
    }

    async fn resolve_existing_dir(&self, path: &str) -> Result<String> {
        if self.existing_dirs.lock().unwrap().iter().any(|d| d == path) {
            Ok(Self::canonical(path))
        } else {
            Err(crate::error::Error::InvalidWorkdir(format!(
                "{path}: no such directory"
            )))
        }
    }

    async fn list_dirs(
        &self,
        path: &str,
        _include_hidden: bool,
    ) -> Result<crate::ports::DirListing> {
        // A minimal listing: the scripted children, if any. The path is
        // canonicalized like `resolve_existing_dir`, and so are the entries'
        // paths, so a caller that must keep the spelling it was configured
        // with is caught relying on the canonical one.
        let canonical = Self::canonical(path);
        let entries = self
            .children
            .lock()
            .unwrap()
            .iter()
            .find(|(dir, _)| dir == path)
            .map(|(_, names)| {
                names
                    .iter()
                    .map(|name| crate::ports::DirEntry {
                        name: name.clone(),
                        path: format!("{canonical}/{name}"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(crate::ports::DirListing {
            path: canonical,
            parent: None,
            entries,
        })
    }

    async fn modified_at(&self, path: &str) -> Result<SystemTime> {
        self.modified
            .lock()
            .unwrap()
            .iter()
            .find(|(dir, _)| dir == path)
            .map(|(_, at)| *at)
            .ok_or_else(|| crate::error::Error::Workspace(format!("{path}: no metadata scripted")))
    }

    async fn remove_dir_tree(&self, path: &str) -> Result<()> {
        self.removed_trees.lock().unwrap().push(path.to_owned());
        Ok(())
    }

    async fn remove_empty_dir(&self, path: &str) -> Result<bool> {
        if self
            .non_empty_dirs
            .lock()
            .unwrap()
            .iter()
            .any(|d| d == path)
        {
            return Ok(false);
        }
        self.removed_empty_dirs
            .lock()
            .unwrap()
            .push(path.to_owned());
        Ok(true)
    }
}
