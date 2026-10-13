use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use crate::image_check::APP_NAME;

/// Why no update can replace the running app's bundle: Delta cannot install
/// it itself ([`InstallError::Unavailable`], whose source this is).
///
/// [`InstallError::Unavailable`]: delta_usecase::InstallError::Unavailable
#[derive(Debug)]
pub enum UnreplaceableBundle {
    /// Where the app runs from could not be told: `std::env::current_exe`
    /// failed with this error, its `source`. Shared, as the installer finds
    /// it once and gives it as the reason for every install.
    Unlocated(Arc<io::Error>),
    /// The executable at `exe` is not inside an app bundle.
    NotInBundle { exe: PathBuf },
    /// macOS runs the app from a read-only copy of it (App Translocation).
    Translocated,
    /// The bundle's path has no parent directory to replace it in.
    NoParent { bundle: PathBuf },
    /// This user may not write to `dir`, where `bundle` is.
    NotWritable { dir: PathBuf, bundle: PathBuf },
}

impl fmt::Display for UnreplaceableBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlocated(_) => f.write_str("could not tell where Delta runs from"),
            Self::NotInBundle { exe } => write!(
                f,
                "Delta does not run from an app bundle ({}), so there is no {APP_NAME} to replace",
                exe.display()
            ),
            Self::Translocated => f.write_str(
                "macOS runs Delta from a read-only copy (App Translocation), since it was \
                 opened from its disk image or the folder it was downloaded to: move Delta to \
                 /Applications",
            ),
            Self::NoParent { bundle } => {
                write!(f, "{} has no parent directory", bundle.display())
            }
            Self::NotWritable { dir, bundle } => write!(
                f,
                "you cannot write to {}, where {} is, so Delta cannot replace it",
                dir.display(),
                bundle.display()
            ),
        }
    }
}

/// Written out rather than derived so that the source of
/// [`UnreplaceableBundle::Unlocated`] is the [`io::Error`] itself, not the
/// [`Arc`] holding it, and a caller may downcast it to [`io::Error`].
impl std::error::Error for UnreplaceableBundle {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unlocated(source) => Some(source.as_ref()),
            Self::NotInBundle { .. }
            | Self::Translocated
            | Self::NoParent { .. }
            | Self::NotWritable { .. } => None,
        }
    }
}
