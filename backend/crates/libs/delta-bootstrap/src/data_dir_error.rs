//! [`DataDirError`]: a directory of the data layout could not be created.

use std::path::PathBuf;

/// A directory of the [`DataLayout`] could not be created.
///
/// [`DataLayout`]: crate::DataLayout
#[derive(Debug, thiserror::Error)]
#[error("could not create Delta's data directory {}: {source}", path.display())]
pub struct DataDirError {
    /// The directory that could not be created.
    pub path: PathBuf,
    /// What creating it failed with.
    pub source: std::io::Error,
}
