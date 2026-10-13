use std::io;
use std::path::PathBuf;

/// Why `pkexec` cannot install the update: Delta cannot install it itself
/// ([`InstallError::Unavailable`], whose source this is).
///
/// [`InstallError::Unavailable`]: delta_usecase::InstallError::Unavailable
#[derive(Debug, thiserror::Error)]
pub enum PkexecUnavailable {
    /// The update helper is not at `helper`.
    #[error("the update helper {} is not installed", helper.display())]
    HelperMissing { helper: PathBuf },
    /// Starting `pkexec` found nothing at its path.
    #[error("{} is not installed", pkexec.display())]
    PkexecMissing {
        pkexec: PathBuf,
        #[source]
        source: io::Error,
    },
    /// `pkexec` ended with 127: the user is not authorized, or no polkit
    /// authentication agent could ask. `said` is `pkexec`'s last line on
    /// stderr, where it printed one.
    #[error(
        "not authorized, or no polkit authentication agent is running{}",
        said.as_deref().map(|said| format!(" (pkexec: {said})")).unwrap_or_default()
    )]
    NotAuthorized { said: Option<String> },
}
