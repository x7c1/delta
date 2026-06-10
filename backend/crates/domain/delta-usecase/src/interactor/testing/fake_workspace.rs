//! In-memory [`Workspace`] fake recording settings writes and modelling a small
//! set of "existing" directories for the workdir-validation path.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::Workspace;

/// Records the session settings written, so tests can assert the path and the
/// rendered JSON the server passed in. Also models a small set of "existing"
/// directories so the workdir-validation path can be exercised: a resolvable
/// path is returned canonicalized (here, prefixed with `/canon` so a test can
/// tell the canonical form apart from the input), anything else is an
/// `InvalidWorkdir`.
#[derive(Default)]
pub(crate) struct FakeWorkspace {
    pub(crate) written: Mutex<Vec<(String, String)>>,
    /// Paths that "exist" as directories; `resolve_existing_dir` accepts these.
    pub(crate) existing_dirs: Mutex<Vec<String>>,
}

impl FakeWorkspace {
    /// The canonical form this fake assigns to a resolvable directory, so tests
    /// can assert the *canonical* path (not the raw input) reaches the launch.
    pub(crate) fn canonical(path: &str) -> String {
        format!("/canon{path}")
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

    async fn list_dirs(&self, path: &str) -> Result<crate::ports::DirListing> {
        // A minimal listing: only used to exercise the browse use case's default
        // and delegation. The path is canonicalized like `resolve_existing_dir`.
        Ok(crate::ports::DirListing {
            path: Self::canonical(path),
            parent: None,
            entries: Vec::new(),
        })
    }
}
