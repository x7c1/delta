use std::path::PathBuf;

/// Why a disk image tool did not do what it was asked.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// The program is not there.
    #[error("{} is not installed", program.display())]
    Missing { program: PathBuf },
    /// The program is there but could not be started.
    #[error("could not run {}: {source}", program.display())]
    Unrunnable {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The program ran and failed; `said` is its last line on stderr, or its
    /// exit status when it printed nothing.
    #[error("{} failed: {said}", program.display())]
    Failed { program: PathBuf, said: String },
}
