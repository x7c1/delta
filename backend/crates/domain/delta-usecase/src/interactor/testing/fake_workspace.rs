//! In-memory [`Workspace`] fake recording settings writes and modelling a small
//! set of "existing" directories for the workdir-validation path.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::Workspace;

/// Records the session settings written, so tests can assert the path and the
/// rendered JSON the server passed in, and the directories it was asked to
/// create. Also models a small set of "existing"
/// directories so the workdir-validation path can be exercised: a resolvable
/// path is returned canonicalized (here, prefixed with `/canon` so a test can
/// tell the canonical form apart from the input), anything else is an
/// `InvalidWorkdir`.
#[derive(Default)]
pub(crate) struct FakeWorkspace {
    pub(crate) written: Mutex<Vec<(String, String)>>,
    /// The directories `create_private_dir` was asked to create, in order.
    pub(crate) created_dirs: Mutex<Vec<String>>,
    /// When set, `create_private_dir` fails instead of recording the
    /// directory, simulating a scratch directory that cannot be created.
    pub(crate) fail_create_dir: Mutex<bool>,
    /// Paths that "exist" as directories; `resolve_existing_dir` accepts these.
    pub(crate) existing_dirs: Mutex<Vec<String>>,
    /// Scripted `list_dirs` children, keyed by the listed path: the bare names
    /// of its subdirectories. A path absent from the map lists no children.
    pub(crate) children: Mutex<Vec<(String, Vec<String>)>>,
    /// The paths `remove_dir_tree` was asked to delete, in order.
    pub(crate) removed_trees: Mutex<Vec<String>>,
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

    async fn create_private_dir(&self, path: &str) -> Result<()> {
        if *self.fail_create_dir.lock().unwrap() {
            return Err(crate::error::Error::Workspace(format!(
                "{path}: could not create the directory"
            )));
        }
        self.created_dirs.lock().unwrap().push(path.to_owned());
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

    async fn remove_dir_tree(&self, path: &str) -> Result<()> {
        self.removed_trees.lock().unwrap().push(path.to_owned());
        Ok(())
    }
}
